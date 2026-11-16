//! Settings: what the session's customisation is, and the few things that are done to it WHOLE - a
//! file loaded into it, the session written to a file, the built-in put back, the session kept for
//! the next start (docs/desktop.md, "Settings"; docs/customisation.md, "Settings and the kept
//! customisation"). The port of customisation/settings_dialog.
//!
//! A page list beside a stack: Customisation, and Updates (below).
//!
//! THE DIALOG DOES NONE OF THE WORK. Every button that changes anything builds the CUSTOMISE line
//! a person would type (katana_cad's customisation_verbs) and hands it to the window's one
//! executor (host::command_runner), so it is echoed in the command log and a refusal is the verb's
//! own, shown as it came. There is no Apply: each control runs its line when it is pressed, and
//! the two automation boxes when they are toggled.
//!
//! IT SHOWS WHAT THE DOCUMENT SAYS, never what a button hoped for. The labels, the boxes and which
//! buttons can be pressed are read from the Document when it says it changed (a DocumentWatcher,
//! once a frame), by its three generations: the customisation's own, the library's and the survey
//! map's - an edit of a session already edited moves only the last two. A box whose line was
//! refused goes back to what the Document has.
//!
//! WHAT IS EDITED HERE IS KEPT. A session that was kept before Import, Reset or a toggled box, and
//! is not after it, is kept again: the dialog runs CUSTOMISE KEEP behind its own line - when the
//! host has a kept file, and only when the first line was carried out. A load typed on the command
//! line or run by a script is not followed so; Settings then says "Not kept" beside Keep.
//!
//! A RESET SO KEPT SETS THE KEPT FILE ASIDE. CUSTOMISE KEEP writes no copy of the built-in: it
//! renames the kept file to <kept file>.bak, and Revert to Kept then has none to read. Nothing is
//! asked first - a question is a modal box - so Reset to Built-in's tip says it before the press,
//! on a kept session, with the file's name and the way back (Import that file, with Replace); and
//! Keep's tip says the same of a session that is the built-in and not kept. Only ONE such file is
//! kept: the next KEEP that finds a kept file puts that one in its place.
//!
//! THE BUILT-IN LIBRARY. A program may carry several built-in customisations (katana_cad's
//! built_ins: Katana Standard, the default, and the alternates a build compiled in). "Built-in
//! library" is a chooser of them and Use puts the chosen one in the place of the session's: the
//! line CUSTOMISE BUILTIN "<name>", followed by KEEP like Reset. The row is hidden when the
//! program has fewer than two to choose between, and Use is disabled while the chosen one is the
//! one the session already is. "Reset to Built-in" is the default's, as ever (CUSTOMISE RESET).
//!
//! REFUSED HERE, with the reason in the status box and the log, and nothing run:
//!
//! * Import, Reset to Built-in, Use and Revert to Kept while the Survey Code Manager's buffer
//!   holds unapplied edits: its later Apply would put the rules it holds back over what was
//!   loaded, without a word;
//! * a file whose path no command line can carry (a double quote, a line break - command_word's
//!   rule), and no file at all. One pair of double quotes round the WHOLE path is no part of it
//!   and is taken off first: a path copied from a file manager comes in them;
//! * Browse in a headless session, which opens no file dialog and names the line to type instead.
//!   Browse is the only thing here that opens one.
//!
//! No button is a default, Enter in a field presses nothing, and no path through the dialog opens
//! a modal box.
//!
//! Object names: settingsDialog; settingsPages (the list), settingsStack, settingsCustomisation
//! and settingsUpdates (the pages); of the first page: settingsActiveName, settingsActiveOrigin,
//! settingsActiveBuiltIn (the built-in the session is, or the one it is based on),
//! settingsActiveDefinitions, settingsActiveSymbols, settingsActiveRules, settingsActiveKept,
//! settingsActiveProblems (what went wrong at start-up; hidden when nothing did),
//! settingsShowNotice (hidden when there is no notice), settingsNotice (read-only, hidden until
//! asked); settingsImportPath, settingsImportBrowse, settingsImportReplace, settingsImport;
//! settingsExportPath, settingsExportBrowse, settingsExport; settingsBuiltInRow (hidden with fewer
//! than two built-ins), settingsBuiltIn (the chooser: each item the built-in's name, the default
//! marked), and settingsBuiltInUse; settingsReset, settingsKeep, settingsRevert;
//! settingsAutoCodes, settingsAutoLinework; settingsLinework (the seven spellings, read-only, each
//! under the word its one editor labels it with), settingsEditLinework; settingsOpenCodes,
//! settingsOpenSymbols; settingsStatus (the line last run and what it answered; a refusal in the
//! error colour), settingsClose.
//!
//! LIFETIME. As the managers: nothing touches the Document once it has gone - which here is the
//! host's drawing having been put in another's place, as the watcher's registration tells.
//! Nothing tells the dialog that it has: the first press finds out, and every control that would
//! act is then disabled and the status says why. What the page last showed stays to be read.
//!
//! THE UPDATES PAGE (settingsUpdates; docs/desktop.md, "Settings", and the UPDATE verbs) is of the
//! program and not of the drawing, so it works with no Document and after one has gone. Like the
//! first page it runs lines and changes nothing itself - with ONE exception, which is the reason
//! it exists:
//!
//! ```text
//! settingsUpdateNote         why this session checks nothing by itself or keeps no update
//!                            settings (hidden when it does)
//! settingsUpdateAuto         "Check for updates automatically"; toggled, it runs UPDATE SET AUTO
//!                            ON or OFF, and then shows what the state has
//! settingsUpdateSends        what a check sends: only the user agent, and that the server sees
//!                            the computer's network address
//! settingsUpdateToken        an access token for private releases, typed or pasted (as dots)
//! settingsUpdateTokenSave    writes the token to the credential store DIRECTLY
//!                            (CredentialStore::write). A token is never a word of a line: lines
//!                            are echoed in the command log. Nothing of it is logged or shown; the
//!                            field is emptied
//! settingsUpdateTokenClear   runs UPDATE TOKEN CLEAR
//! settingsUpdateTokenState   whether a token is stored, and where
//! settingsUpdateSkipped      the version UPDATE SKIP set aside, or that none is; nothing when
//!                            nobody can say (the state file does not read)
//! settingsUpdateUnskip       "Offer the skipped version again": runs UPDATE SKIP NONE
//! settingsUpdateBlocked      whether a version is set aside because it was installed here, did
//!                            not start and was put back, or that none is; nothing when nobody can
//!                            say
//! settingsUpdateUnblock      "Offer the version that was put back again": runs UPDATE BLOCKED
//!                            CLEAR, as Unskip runs its line
//! ```
//!
//! Each of the two buttons is off when its list is known to be empty, and ON when nobody can say
//! what the list holds: the line is then run, and the verbs answer - with why they refuse it, for
//! a state file that does not read. The page is there whatever the dialog was given; with no
//! SettingsUpdates its controls are disabled.
//!
//! THE RUST SHAPE. The C++'s context holds the Document by a pointer that may dangle, a log, and
//! whether the session is headless; here the drawing, the log and that question are the host's,
//! lent at each call, and the context says only whether the dialog reads the drawing at all
//! (`reads_drawing`, the C++'s pointer being null or not). The system's file dialog is asked
//! through the context (`choose_file`), so that a test answers it where the C++ tests caught the
//! QFileDialog as it opened. What a Qt listener did at once is done at the dialog's next look.

use std::rc::Rc;

use egui::{RichText, Ui};
use katana_cad::Document;
use katana_cad::customisation_host::{
    BuiltInChoice, BuiltInEntry, KEPT_CUSTOMISATION_VARIABLE, built_in_choices, built_ins,
};
use katana_cad::customisation_state::{CustomisationOrigin, CustomisationState};
use katana_cad::style_catalogue::symbol_definition_count;
use katana_core::ErrorCode;
use katana_entity::linework_codes::{LineworkCodes, linework_code_members};
use katana_update::{CredentialStore, TOKEN_ENVIRONMENT_VARIABLE};

use super::code_manager_support::{
    customisation_file_filter, typed_customisation_file_word, typed_file_path,
};
use super::document_watcher::DocumentWatcher;
use super::linework_labels::linework_control_label;
use crate::command_log::folded;
use crate::command_word::command_word;
use crate::dialog_layout as layout;
use crate::drive::controls::{self, Choice, LineOptions, TextOptions};
use crate::drive::registry::{ControlKind, ControlRecord};
use crate::host::{Host, VerbOutcome};
use crate::ifc_dialogs::{get_open_file_name, get_save_file_name};
use crate::script_runner::from_native_separators;
use crate::shell::DialogFrame;
use crate::shell::dialog;
use crate::survey::survey_dialogs::{hidden, offstage};
use crate::theme;

/// What the Updates page shows, and the one thing it does without a line. Every member has a
/// default a test can leave alone; the window fills it from its update workbench
/// (UpdateWorkbench::settings_updates).
#[derive(Clone, Default)]
pub struct SettingsUpdates {
    /// "Check for updates automatically" as the per-user state has it. Asked whenever the page is
    /// filled, and after each of its lines. None: the box is disabled.
    pub automatic: Option<Rc<dyn Fn() -> bool>>,
    /// The version UPDATE SKIP set aside, empty when none is. Asked likewise. It answers None when
    /// nobody can say - the state file does not read - and that and no function mean the same: the
    /// page says nothing of it and the button stays on, so that the verb answers.
    pub skipped_version: Option<Rc<dyn Fn() -> Option<String>>>,
    /// Whether a version is set aside because it was installed here, did not start and was put
    /// back (the state's list of blocked releases holds one that can still be offered to this
    /// install, which the window decides: update_dialog's update_put_back_of_state; which version
    /// is not kept, only which build). Asked likewise, and None and no function mean the same.
    pub put_back: Option<Rc<dyn Fn() -> Option<bool>>>,
    /// Where the access token is kept: Save writes it here and the page says whether one is
    /// stored - never the token itself. None: Save is disabled.
    pub credentials: Option<Rc<dyn CredentialStore>>,
    /// Told once Save has changed the store, so the next check reads it again.
    pub token_changed: Option<Rc<dyn Fn()>>,
    /// What a check sends as its user agent, "Katana/0.2.0 (win64)". Empty: the sentence gives the
    /// shape, "Katana/<version> (<target>)".
    pub user_agent: String,
    /// Who the check asks ("GitHub"). Empty: "the release server".
    pub server: String,
    /// One sentence, shown first: why this session checks nothing by itself or keeps no update
    /// settings (a headless run, a development build, KATANA_UPDATE_DISABLE). Empty: nothing is
    /// shown.
    pub note: String,
}

/// A line handed to the window's one executor, with the window: what `Host::run` is, as a value a
/// test or an owner can stand in for.
pub type SettingsRun = Box<dyn FnMut(&mut dyn Host, &str) -> VerbOutcome>;
/// One of the editors the page opens, handed the window when it runs.
pub type SettingsOpen = Box<dyn FnMut(&mut dyn Host)>;

/// What Browse asks the system's file dialog for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileDialogAsk {
    /// A file to write (QFileDialog::getSaveFileName), else one to read (getOpenFileName).
    pub save: bool,
    /// The dialog's title.
    pub title: String,
    /// Where it opens: the path the field holds, read as Import reads it.
    pub start: String,
    /// The filters it offers, as Qt writes them ("Name (*.a *.b);;All files (*)").
    pub filter: String,
}

/// The file a person chose in the system's file dialog, as the platform writes it; empty when the
/// dialog was cancelled.
pub type SettingsChooseFile = Box<dyn FnMut(&FileDialogAsk) -> String>;

/// What the dialog is given by whoever opens it. Every member has a default a test can leave
/// alone: with no drawing nothing is shown and nothing runs, and a button whose callback is empty
/// is disabled.
#[derive(Default)]
pub struct SettingsContext {
    /// Whether the dialog shows the host's drawing's customisation (the C++'s `document`, which a
    /// caller may leave null). The drawing may go before the dialog.
    pub reads_drawing: bool,
    /// The window's one executor. None: a press says nothing can run its line.
    pub run: Option<SettingsRun>,
    /// True while the Survey Code Manager's buffer holds edits that are not on the drawing. Asked
    /// at each press. None: it holds none.
    pub code_manager_dirty: Option<Box<dyn Fn() -> bool>>,
    /// Open the Survey Code Manager, the same at its Linework tab - the ONE editor of the control
    /// codes - and the Symbol Library.
    pub open_code_manager: Option<SettingsOpen>,
    pub open_linework: Option<SettingsOpen>,
    pub open_symbol_library: Option<SettingsOpen>,
    /// What the session's host offers (katana_cad's CustomisationHost): whether there is a
    /// built-in customisation to reset to, and the kept file as text, for showing - empty when
    /// this session keeps none. The file need not exist.
    pub has_built_in: bool,
    pub kept_file: String,
    /// The built-ins the chooser offers (katana_cad's built_ins_of the session's host). None:
    /// those compiled into the program, katana_cad's built_ins - what the window wants. A test
    /// gives it to stand in for the build, so that it is the same test on a machine whose build
    /// compiled in other files.
    pub built_ins: Option<Box<dyn Fn() -> Vec<BuiltInEntry>>>,
    /// The system's file dialog, for Browse. None: the system's own (never asked in a headless
    /// session).
    pub choose_file: Option<SettingsChooseFile>,
    /// The Updates page's. The Format workbench, which makes the dialog, knows nothing of updates:
    /// the window hands these over with `set_updates` each time the dialog is opened.
    pub updates: SettingsUpdates,
}

/// The executor every window gives its dialogs: `Host::run`.
pub fn run_through_host() -> SettingsRun {
    Box::new(|host: &mut dyn Host, line: &str| host.run(line))
}

/// How the access token's field is drawn: as dots, since a token typed with someone looking on is
/// still a secret.
pub const TOKEN_FIELD: LineOptions<'static> = LineOptions {
    placeholder: "paste an access token to store it",
    read_only: false,
    password: true,
    width: None,
};

/// How the notice is drawn: read, never typed into.
pub const NOTICE_BOX: TextOptions = TextOptions {
    read_only: true,
    rows: 6,
    monospace: false,
};

const STATUS_PLACEHOLDER: &str = "The line each button runs is shown here, with what it answered.";
const IMPORT_PLACEHOLDER: &str = "a Katana customisation file to load";
const EXPORT_PLACEHOLDER: &str = "the file to write the customisation to";

const SHOW_NOTICE_TIP: &str =
    "What the author of the customisation says of it and of its use, as its file carries it";
const IMPORT_REPLACE_TIP: &str = "Put the file's definitions in the place of the whole library \
     and its rules in the place of all the survey codes, each kind it brings, where a merge \
     replaces a definition by its name and a code's rules by their section (CUSTOMISE REPLACE)";
const IMPORT_TIP: &str = "Load the file into the session's customisation (CUSTOMISE and the file)";
const EXPORT_TIP: &str = "Write the session's whole customisation to the file; a file that is \
     there is written over, and the answer says so (CUSTOMISE EXPORT and the file)";
const AUTO_CODES_TIP: &str = "Code the points a survey import creates, by the survey code rules \
     (CUSTOMISE SET auto.codes)";
const AUTO_LINEWORK_TIP: &str = "Then join the coded points into lines, by the linework control \
     codes (CUSTOMISE SET auto.linework)";
const OPEN_CODES_TIP: &str = "Open the editor of the survey code rules";
const OPEN_SYMBOLS_TIP: &str = "Open the browser and editor of the symbols";
const LINEWORK_TIP: &str = "What is typed after a point's code to start, end or close a line, \
     begin and end a curve, join to another point or make a rectangle";
const EDIT_LINEWORK_TIP: &str = "Open the editor of the survey codes at its Linework tab, the one \
     place the control codes are changed";
const UPDATE_AUTO_TIP: &str = "Ask once a day, when the window is open, whether a newer Katana \
     exists, and show it in the status bar when one does; nothing is downloaded or installed \
     unasked (UPDATE SET AUTO ON or OFF). Help > Check for Updates asks at any time";
const TOKEN_WHY: &str = "Needed only when the releases are private: a read-only token, which is \
     sent to the release server and to nothing else. Leave it empty otherwise.";
const TOKEN_CLEAR_TIP: &str = "Remove the stored access token (UPDATE TOKEN CLEAR)";
const UNSKIP_TIP: &str =
    "Forget which version was skipped, so that the next check offers it (UPDATE SKIP NONE)";
const UNBLOCK_TIP: &str = "Forget which versions were put back because they did not start here, \
     so that the next check offers the newest again and it can be installed (UPDATE BLOCKED CLEAR)";

/// The pages, in the order of the list: each one's title and the object name of its page.
const PAGES: [(&str, &str); 2] = [
    ("Customisation", "settingsCustomisation"),
    ("Updates", "settingsUpdates"),
];
const UPDATES_PAGE: usize = 1;

fn lines(each: &[String]) -> String {
    each.join("\n")
}

/// Where the session's customisation came from, in plain words: a reply names an origin by one
/// word, which is for a script to read.
fn origin_words(origin: CustomisationOrigin) -> &'static str {
    match origin {
        CustomisationOrigin::None => "None",
        CustomisationOrigin::BuiltIn => "Built in",
        CustomisationOrigin::Kept => "Kept by you",
        CustomisationOrigin::Loaded => "Loaded this session",
        CustomisationOrigin::Edited => "Edited this session",
    }
}

/// What the START of this session found, for the row under "What is active": each thing that went
/// wrong, a sentence as katana_cad's start_customisation wrote it - a kept file that did not read
/// and what started in its place, a built-in that did not parse - and, last, that the kept
/// customisation was made from another built-in than this program has. The Document keeps both,
/// because the window says them once, in a log that has scrolled away by the time a person opens
/// Settings to ask why their own customisation is not the one loaded.
///
/// They are of the start and nothing since changes them: a session reset, or loaded over, still
/// says what its start found.
fn start_problems_of(state: &CustomisationState) -> Vec<String> {
    let mut problems = state.start_problems.clone();
    if state.kept_from_another_built_in {
        // The window's own start-up words, with the button a person has here in the place of the
        // line to type.
        problems.push(String::from(
            "The kept customisation was made from another built-in customisation than this \
             program has; Reset to Built-in gives this program's.",
        ));
    }
    problems
}

/// The author's notice a person is shown: the customisation's own, then that of each customisation
/// that went into the session and kept one of its own, under its name. A source whose notice IS
/// the customisation's own - a file lists itself as its one source - is not said twice.
fn notice_of(state: &CustomisationState) -> String {
    let mut parts = Vec::new();
    if !state.notice.is_empty() {
        parts.push(lines(&state.notice));
    }
    for source in &state.sources {
        if source.notice.is_empty() || source.notice == state.notice {
            continue;
        }
        parts.push(format!("From {}:\n{}", source.name, lines(&source.notice)));
    }
    parts.join("\n\n")
}

/// The seven control codes as the session spells them, in the order of katana_entity's
/// linework_code_members, each under the word a person reads its control by - the ONE list of
/// those words (linework_labels), which is the controls' one editor's too: the same control under
/// another word here would be two names for one thing. An empty spelling is a control switched
/// off.
fn linework_words(codes: &LineworkCodes) -> String {
    let words: Vec<String> = linework_code_members()
        .iter()
        .map(|member| {
            let spelling = member.spelling(codes);
            format!(
                "{}: {}",
                linework_control_label(member.name),
                if spelling.is_empty() {
                    "(off)"
                } else {
                    spelling
                }
            )
        })
        .collect();
    words.join(", ")
}

fn rules_words(rules: usize, codes: usize) -> String {
    if rules == 0 {
        return String::from("0");
    }
    format!(
        "{rules} over {codes} {}",
        if codes == 1 { "code" } else { "codes" }
    )
}

/// "1 code", "2 codes": the count and the noun for it.
fn count_words(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

/// One built-in as the chooser lists it: its name, and that it is the default.
fn built_in_item_text(choice: &BuiltInChoice) -> String {
    if choice.is_default {
        format!("{} (default)", choice.name)
    } else {
        choice.name.clone()
    }
}

/// What a person is told of a built-in before choosing it: its own description, what it holds and
/// where it was compiled in from - or why it cannot be used.
fn built_in_tip(choice: &BuiltInChoice) -> String {
    if !choice.problem.is_empty() {
        return choice.problem.clone();
    }
    let mut parts = Vec::new();
    if !choice.description.is_empty() {
        parts.push(choice.description.clone());
    }
    parts.push(format!(
        "{}, {} over {}",
        count_words(choice.definitions, "definition", "definitions"),
        count_words(choice.rules, "survey code rule", "survey code rules"),
        count_words(choice.codes, "code", "codes")
    ));
    if !choice.source.is_empty() {
        parts.push(format!("Compiled in from {}", choice.source));
    }
    parts.join("\n")
}

/// The "Built-in" row under "What is active": the built-in the session IS, or the one a kept or
/// edited copy of it was made from.
fn active_built_in_words(choices: &[BuiltInChoice], state: &CustomisationState) -> String {
    if let Some(choice) = choices.iter().find(|choice| choice.is_active) {
        return built_in_item_text(choice);
    }
    match &state.based_on {
        Some(base) => format!("Based on {}", base.name),
        None => String::from("None"),
    }
}

fn no_kept_file() -> String {
    format!(
        "This session has no kept file; the environment variable {KEPT_CUSTOMISATION_VARIABLE} \
         names one."
    )
}

/// The Kept row. With a kept file it says whether the session is what that file gives the next
/// start. With none there is nothing to be kept: the next start gives the built-in (or nothing, in
/// a program without one), and the row says that instead of the contradiction "kept ... no kept
/// file".
fn kept_words(kept: bool, kept_file: &str, has_built_in: bool) -> String {
    if kept_file.is_empty() {
        return format!(
            "Nothing is kept: the next start gives {}. The environment variable \
             {KEPT_CUSTOMISATION_VARIABLE} names a file to keep one in.",
            if has_built_in {
                "the built-in customisation"
            } else {
                "no customisation"
            }
        );
    }
    format!(
        "{}\nKept file: {kept_file}",
        if kept {
            "Kept: the next start gives this customisation."
        } else {
            "Not kept: the next start does not give this customisation."
        }
    )
}

/// Where CUSTOMISE KEEP leaves the kept file that was there: beside it, under this name, the one
/// before it of that name gone (docs/customisation.md, "The host").
fn set_aside_file(kept_file: &str) -> String {
    format!("{kept_file}.bak")
}

/// What Reset to Built-in says of itself BEFORE it is pressed, since nothing is asked after (a
/// question is a modal box). On a kept session the press costs the kept file: the reset is kept
/// like every edit made here, and CUSTOMISE KEEP keeps a session that IS the built-in by setting
/// the kept file aside, after which CUSTOMISE REVERT has none to read. A session that is not kept
/// is not followed by KEEP, and its kept file stands.
fn reset_tip(has_built_in: bool, kept: bool, kept_file: &str) -> String {
    if !has_built_in {
        return String::from("This program has no built-in customisation to reset to.");
    }
    let mut tip = String::from(
        "Put the program's built-in customisation in the place of the session's (CUSTOMISE RESET)",
    );
    if kept && !kept_file.is_empty() {
        tip.push_str(&format!(
            ".\nThis session is the kept one, so its reset is kept too: the kept file, when there \
             is one, is set aside as {}, and Revert to Kept then has none to read.\nImport that \
             file with Replace instead of merging ticked to have it back.",
            set_aside_file(kept_file)
        ));
    }
    tip
}

/// What Use says of itself BEFORE it is pressed, for the same reason as Reset (no question is
/// asked after). The press is followed by CUSTOMISE KEEP when the session is the kept one, and the
/// file that was kept does not stay where it is: the chosen built-in is written in its place and
/// what was there goes to `<kept file>.bak`, replacing the one before it; or, when the chosen one
/// is the built-in of the run, which every start gives when nothing is kept, nothing is written
/// and the kept file is set aside the same way. The press after that replaces the `.bak` again, so
/// a kept customisation of the person's own is in neither file by the second press. A session
/// that is not kept is followed by nothing, and its kept file stands.
fn use_tip(kept: bool, chosen_is_the_runs_own: bool, kept_file: &str) -> String {
    let tip = "Put the chosen built-in customisation in the place of the session's (CUSTOMISE \
               BUILTIN)";
    if kept_file.is_empty() {
        return String::from(tip);
    }
    if !kept {
        return format!(
            "{tip}.\nThis session is not the kept one, so the choice is not kept: the kept file \
             stands."
        );
    }
    let aside = set_aside_file(kept_file);
    if chosen_is_the_runs_own {
        return format!(
            "{tip}.\nThis session is the kept one, so the choice is kept too: that is the \
             built-in every start gives when nothing is kept, so the kept file is set aside as \
             {aside}, replacing the file of that name, and Revert to Kept then has none to \
             read.\nImport that file with Replace instead of merging ticked to have it back."
        );
    }
    format!(
        "{tip}.\nThis session is the kept one, so the choice is kept too: the kept file is \
         written with it, and what was there is set aside as {aside}, replacing the file of that \
         name.\nImport that file with Replace instead of merging ticked to have it back."
    )
}

/// What Keep says of itself: why it cannot be pressed, or what the press does. A session whose
/// origin is still BuiltIn is the built-in and nothing more - any edit makes it Edited, any load
/// Loaded - and CUSTOMISE KEEP writes no copy of that: it sets the kept file aside, so that the
/// next start gives the built-in, and a later edition of it is not hidden by a copy of this one.
fn keep_tip(state: &CustomisationState, kept_file: &str) -> String {
    if kept_file.is_empty() {
        return no_kept_file();
    }
    if state.kept {
        return String::from("The session is already kept: the next start gives it.");
    }
    if state.origin == CustomisationOrigin::BuiltIn {
        return format!(
            "The session is the built-in customisation, which every start gives when nothing is \
             kept: nothing is written, and the kept file, when there is one, is set aside as {} \
             (CUSTOMISE KEEP)",
            set_aside_file(kept_file)
        );
    }
    String::from(
        "Write the session's customisation to the kept file, which the next start reads \
         (CUSTOMISE KEEP)",
    )
}

/// A count of one or more ASCII digits, as QString::toLongLong reads it (0 for one too long).
fn count_of(digits: &str) -> Option<i64> {
    if digits.is_empty() || !digits.bytes().all(|digit| digit.is_ascii_digit()) {
        return None;
    }
    Some(digits.parse().unwrap_or(0))
}

/// "1 name", "2 names".
fn counted_names(count: i64) -> String {
    format!("{count} {}", if count == 1 { "name" } else { "names" })
}

/// A load's reply with the names the program's own built-in leaves undefined said once, as a
/// count: a load reports every name the rules ask for that nothing defines, and the built-in's own
/// few (`in_built_in=yes`) would be listed after every load of anything, as though that file had
/// broken them.
fn with_built_in_undefined_as_a_count(reply: &str) -> String {
    let mut kept = Vec::new();
    for line in reply.split('\n') {
        if line.starts_with("undefined name=") && line.ends_with(" in_built_in=yes") {
            continue;
        }
        // "undefined names=<n> in_built_in=<n>", the whole line.
        let header = line
            .strip_prefix("undefined names=")
            .and_then(|rest| rest.split_once(" in_built_in="))
            .and_then(|(names, own)| Some((count_of(names)?, count_of(own)?)));
        let Some((names, own)) = header else {
            kept.push(String::from(line));
            continue;
        };
        kept.push(if own == names {
            format!("{} undefined (as in the built-in)", counted_names(names))
        } else {
            format!(
                "{} undefined, {own} of them as in the built-in",
                counted_names(names)
            )
        });
    }
    kept.join("\n")
}

/// A line and what it answered, as the status shows them: the line as the command log echoes it,
/// then the verb's own words - its reply, or what it was refused for.
fn answered(line: &str, outcome: &VerbOutcome) -> String {
    let mut shown = format!("> {line}\n");
    if outcome.ok {
        if outcome.reply.is_empty() {
            shown.push_str("Done.");
        } else {
            shown.push_str(&with_built_in_undefined_as_a_count(&outcome.reply));
        }
    } else {
        shown.push_str("Refused: ");
        shown.push_str(if outcome.error.is_empty() {
            "the command was refused."
        } else {
            &outcome.error
        });
    }
    shown
}

// ---- the Updates page's words -------------------------------------------------------------------

/// What a check sends, and what the server it asks sees whatever is sent: the one thing about a
/// person's computer an update check gives away, said where the check is switched on and off.
fn update_sends_words(updates: &SettingsUpdates) -> String {
    let server = if updates.server.is_empty() {
        "the release server"
    } else {
        &updates.server
    };
    let user_agent = if updates.user_agent.is_empty() {
        "Katana/<version> (<target>)"
    } else {
        &updates.user_agent
    };
    format!(
        "A check asks {server} for the newest version. It sends only \"{user_agent}\" as its user \
         agent - nothing about you, your files or your drawings - and, as with every website, \
         {server} sees this computer's network address."
    )
}

/// Whether a token is stored and where - never the token. A store that cannot be read says so in
/// its own words, which name the place and not the value (katana_update's credential).
fn update_token_words(store: Option<&dyn CredentialStore>) -> String {
    let Some(store) = store else {
        return format!(
            "This session keeps no access token; the environment variable \
             {TOKEN_ENVIRONMENT_VARIABLE} gives a run one."
        );
    };
    match store.read() {
        Ok(_) => format!("An access token is stored in the {}.", store.r#where()),
        Err(error) if error.code == ErrorCode::NotFound => {
            String::from("No access token is stored.")
        }
        Err(error) => error.message,
    }
}

fn update_skipped_words(version: &str) -> String {
    if version.is_empty() {
        String::from("No version is skipped.")
    } else {
        format!("Katana {version} is skipped: it is not offered.")
    }
}

/// Whether a version is set aside, without its number: the state keeps which BUILD was put back (a
/// release's sequence), and no version to say.
fn update_put_back_words(put_back: bool) -> &'static str {
    if put_back {
        "A version that was installed here did not start and was put back: it is not offered."
    } else {
        "No version was put back."
    }
}

/// What a line that changed the customisation is followed by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Then {
    Nothing,
    /// CUSTOMISE KEEP, when the session was kept before the line and is not after it.
    KeepWhatWasKept,
}

/// Which controls of the first page can be pressed. Every one is on until the page is filled, as a
/// widget is until it is disabled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Enabled {
    import_path: bool,
    import_browse: bool,
    import_replace: bool,
    import: bool,
    export_path: bool,
    export_browse: bool,
    export: bool,
    built_in_choice: bool,
    built_in_use: bool,
    reset: bool,
    keep: bool,
    revert: bool,
    auto_codes: bool,
    auto_linework: bool,
    edit_linework: bool,
    open_codes: bool,
    open_symbols: bool,
}

impl Enabled {
    const fn all(on: bool) -> Self {
        Self {
            import_path: on,
            import_browse: on,
            import_replace: on,
            import: on,
            export_path: on,
            export_browse: on,
            export: on,
            built_in_choice: on,
            built_in_use: on,
            reset: on,
            keep: on,
            revert: on,
            auto_codes: on,
            auto_linework: on,
            edit_linework: on,
            open_codes: on,
            open_symbols: on,
        }
    }
}

/// What the Updates page shows, as `show_updates` last filled it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct UpdatesPage {
    note: String,
    auto: bool,
    auto_enabled: bool,
    sends: String,
    token: String,
    token_enabled: bool,
    token_save_enabled: bool,
    token_save_tip: String,
    token_clear_enabled: bool,
    token_state: String,
    skipped: String,
    unskip_enabled: bool,
    blocked: String,
    unblock_enabled: bool,
}

/// File > Settings (see the module comment).
pub struct SettingsDialog {
    frame: DialogFrame,
    visible: bool,
    context: SettingsContext,
    /// The Document's generations the page last showed.
    shown_customisation: u64,
    shown_library: u64,
    shown_survey_map: u64,

    /// The page in front: a row of the list.
    page: Option<usize>,
    name: String,
    origin: String,
    definitions: String,
    symbols: String,
    rules: String,
    active_built_in: String,
    kept: String,
    problems: String,
    show_notice: bool,
    show_notice_shown: bool,
    notice: String,
    notice_shown: bool,
    import_path: String,
    import_replace: bool,
    export_path: String,
    built_in_row_shown: bool,
    /// The chooser: what each item shows, the built-in's name each stands for, and its tip.
    built_in_items: Vec<String>,
    built_in_names: Vec<String>,
    built_in_tips: Vec<String>,
    built_in: Choice,
    /// The built-ins the chooser lists, as it last listed them, in its order.
    built_in_choices: Vec<BuiltInChoice>,
    built_in_use_tip: String,
    reset_tip: String,
    keep_tip: String,
    revert_tip: String,
    auto_codes: bool,
    auto_linework: bool,
    linework: String,
    enabled: Enabled,
    updates: UpdatesPage,
    status: String,
    status_error: bool,

    /// How everything here knows the drawing is still there; None for a dialog that reads none.
    watcher: Option<DocumentWatcher>,
}

/// The dialog and the window, lent together to the dialog frame's body and footer.
struct Lent<'a, 'h> {
    dialog: &'a mut SettingsDialog,
    host: &'a mut (dyn Host + 'h),
}

/// A value's label: a name, a path and a notice are a file's own words, never markup, and egui
/// draws a label's text as it is.
fn value_label(ui: &mut Ui, name: &str, text: &str) -> egui::Response {
    controls::label_as(
        ui,
        name,
        text,
        egui::Label::new(text).wrap().selectable(true),
    )
}

/// A QGroupBox: its title over what it holds - drawn as a section (a heading, no box), so the page
/// reads as one column rather than a stack of frames of different widths.
fn group(ui: &mut Ui, title: &str, add: impl FnOnce(&mut Ui)) {
    dialog::section(ui, title);
    add(ui);
}

/// The width of the page beside the list of pages.
const PAGE_WIDTH: f32 = 560.0;

impl SettingsDialog {
    /// The dialog over `context`, filled from `host`'s drawing when the context reads one.
    pub fn new(context: SettingsContext, host: &dyn Host) -> Self {
        let watcher = context
            .reads_drawing
            .then(|| DocumentWatcher::new(host.document()));
        let mut dialog = Self {
            frame: DialogFrame::new("settingsDialog", "Settings"),
            visible: false,
            context,
            shown_customisation: 0,
            shown_library: 0,
            shown_survey_map: 0,
            page: Some(0),
            name: String::new(),
            origin: String::new(),
            definitions: String::new(),
            symbols: String::new(),
            rules: String::new(),
            active_built_in: String::new(),
            kept: String::new(),
            problems: String::new(),
            show_notice: false,
            show_notice_shown: false,
            notice: String::new(),
            notice_shown: false,
            import_path: String::new(),
            import_replace: false,
            export_path: String::new(),
            built_in_row_shown: false,
            built_in_items: Vec::new(),
            built_in_names: Vec::new(),
            built_in_tips: Vec::new(),
            built_in: Choice::default(),
            built_in_choices: Vec::new(),
            built_in_use_tip: String::new(),
            reset_tip: String::new(),
            keep_tip: String::new(),
            revert_tip: String::new(),
            auto_codes: false,
            auto_linework: false,
            linework: String::new(),
            enabled: Enabled::all(true),
            updates: UpdatesPage::default(),
            status: String::new(),
            status_error: false,
            watcher,
        };
        dialog.refresh(host);
        // After refresh, which says in the status when there is no drawing: the Updates page is of
        // the program and is filled either way.
        dialog.show_updates();
        dialog
    }

    /// The C++ object name.
    pub fn object_name(&self) -> &str {
        self.frame.name()
    }

    pub fn window_title(&self) -> &str {
        self.frame.title()
    }

    pub fn is_modal(&self) -> bool {
        self.frame.is_modal()
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn show(&mut self) {
        self.visible = true;
    }

    /// QDialog::close: hidden, and kept by whoever opened it, with what was typed.
    pub fn close(&mut self) {
        self.visible = false;
    }

    /// What settingsStatus says, and whether in the colour of a refusal.
    pub fn status(&self) -> &str {
        &self.status
    }

    pub fn status_is_error(&self) -> bool {
        self.status_error
    }

    /// The colour settingsActiveProblems and a refusal in settingsStatus are written in.
    pub fn error_colour(&self) -> crate::canvas::Color {
        theme::error()
    }

    /// The built-in each item of the chooser stands for, by name (the items' data).
    pub fn built_in_names(&self) -> &[String] {
        &self.built_in_names
    }

    /// What a person is told of each built-in before choosing it (the items' tips).
    pub fn built_in_tips(&self) -> &[String] {
        &self.built_in_tips
    }

    /// The chooser's item chosen; None for none.
    pub fn built_in_index(&self) -> Option<usize> {
        self.built_in.index
    }

    /// The tooltip of the control `name`; empty for one that has none.
    pub fn tip(&self, name: &str) -> String {
        let tip: &str = match name {
            "settingsShowNotice" => SHOW_NOTICE_TIP,
            "settingsImportReplace" => IMPORT_REPLACE_TIP,
            "settingsImport" => IMPORT_TIP,
            "settingsExport" => EXPORT_TIP,
            "settingsBuiltInUse" => &self.built_in_use_tip,
            "settingsReset" => &self.reset_tip,
            "settingsKeep" => &self.keep_tip,
            "settingsRevert" => &self.revert_tip,
            "settingsAutoCodes" => AUTO_CODES_TIP,
            "settingsAutoLinework" => AUTO_LINEWORK_TIP,
            "settingsOpenCodes" => OPEN_CODES_TIP,
            "settingsOpenSymbols" => OPEN_SYMBOLS_TIP,
            "settingsLinework" => LINEWORK_TIP,
            "settingsEditLinework" => EDIT_LINEWORK_TIP,
            "settingsUpdateAuto" => UPDATE_AUTO_TIP,
            "settingsUpdateTokenSave" => &self.updates.token_save_tip,
            "settingsUpdateTokenClear" => TOKEN_CLEAR_TIP,
            "settingsUpdateUnskip" => UNSKIP_TIP,
            "settingsUpdateUnblock" => UNBLOCK_TIP,
            _ => "",
        };
        String::from(tip)
    }

    /// Every text the dialog can show a person that is not a named control's own text or value:
    /// its title, the pages' and the boxes' titles, the captions, the placeholders, and every tip
    /// as it stands now - the chooser's items' among them.
    pub fn words(&self) -> Vec<String> {
        let mut words: Vec<String> = [
            self.frame.title(),
            "What is active",
            "Name",
            "Origin",
            "Built-in",
            "Definitions",
            "Symbols among them",
            "Survey code rules",
            "Kept",
            "Import",
            "Export",
            "File",
            "Built-in and kept",
            "Built-in library",
            "When survey data comes in",
            "Editors",
            "Linework control codes",
            "Checking",
            "Access token",
            TOKEN_WHY,
            "Skipped version",
            "Version that was put back",
            STATUS_PLACEHOLDER,
            IMPORT_PLACEHOLDER,
            EXPORT_PLACEHOLDER,
            TOKEN_FIELD.placeholder,
        ]
        .into_iter()
        .map(String::from)
        .collect();
        words.extend(PAGES.iter().map(|(title, _)| String::from(*title)));
        for name in [
            "settingsShowNotice",
            "settingsImportReplace",
            "settingsImport",
            "settingsExport",
            "settingsBuiltInUse",
            "settingsReset",
            "settingsKeep",
            "settingsRevert",
            "settingsAutoCodes",
            "settingsAutoLinework",
            "settingsOpenCodes",
            "settingsOpenSymbols",
            "settingsLinework",
            "settingsEditLinework",
            "settingsUpdateAuto",
            "settingsUpdateTokenSave",
            "settingsUpdateTokenClear",
            "settingsUpdateUnskip",
            "settingsUpdateUnblock",
        ] {
            words.push(self.tip(name));
        }
        words.extend(self.built_in_tips.iter().cloned());
        words.retain(|word| !word.is_empty());
        words
    }

    /// Whether the drawing the dialog was opened on is still the host's.
    fn alive(&self) -> bool {
        self.context.reads_drawing
            && self
                .watcher
                .as_ref()
                .is_some_and(DocumentWatcher::document_alive)
    }

    /// The dialog's look at the drawing: the page read again when the Document said it changed and
    /// what the page shows has moved. The Document also notifies for a selection, a save, a
    /// command: those change nothing here.
    fn look(&mut self, host: &dyn Host) {
        if !self.alive() {
            return;
        }
        let document = host.document();
        let notified = self
            .watcher
            .as_mut()
            .is_some_and(|watcher| watcher.deliver(document).is_some());
        if notified
            && (document.customisation_generation() != self.shown_customisation
                || document.library_generation() != self.shown_library
                || document.survey_map_generation() != self.shown_survey_map)
        {
            self.refresh(host);
        }
    }

    // ---- what the Document says -----------------------------------------------------------------

    /// Everything the page shows, read from the Document again: what the watcher's delivery does
    /// when the Document says it changed.
    pub fn refresh(&mut self, host: &dyn Host) {
        if !self.alive() {
            self.show_closed();
            return;
        }
        let document = host.document();
        let state = document.customisation_state();
        self.shown_customisation = document.customisation_generation();
        self.shown_library = document.library_generation();
        self.shown_survey_map = document.survey_map_generation();

        // A session edited before anything was installed has rules or definitions and no name;
        // one nothing was ever put into has neither.
        self.name = if !state.name.is_empty() {
            state.name.clone()
        } else if state.origin == CustomisationOrigin::None {
            String::from("No customisation is loaded")
        } else {
            String::from("Not named")
        };
        self.origin = String::from(origin_words(state.origin));
        self.definitions = format!("{}", document.style_library().size());
        self.symbols = format!("{}", symbol_definition_count(document));
        self.rules = rules_words(
            document.survey_map().size(),
            document.survey_map().keys().len(),
        );
        self.kept = kept_words(
            state.kept,
            &self.context.kept_file,
            self.context.has_built_in,
        );

        // Hidden while it is empty: a session whose start found nothing.
        self.problems = start_problems_of(state).join("\n");

        // The toggle is there only while there is a notice to show, and what it shows follows the
        // Document like the rest.
        self.notice = notice_of(state);
        self.show_notice_shown = !self.notice.is_empty();
        if self.notice.is_empty() {
            self.show_notice = false;
        }
        self.notice_shown = self.show_notice && !self.notice.is_empty();

        self.linework = linework_words(&state.linework);
        self.show_automation(host);
        self.show_built_ins(host);

        let enabled = &mut self.enabled;
        enabled.import_path = true;
        enabled.import_browse = true;
        enabled.import_replace = true;
        enabled.import = true;
        enabled.export_path = true;
        enabled.export_browse = true;
        enabled.export = true;
        enabled.auto_codes = true;
        enabled.auto_linework = true;
        // Disabled with the reason, where the line could only be refused for what the session's
        // host does not have - or, for Keep, has nothing to do. And where a press would set the
        // kept file aside, the tip says so first.
        let has_kept_file = !self.context.kept_file.is_empty();
        enabled.reset = self.context.has_built_in;
        self.reset_tip = reset_tip(
            self.context.has_built_in,
            state.kept,
            &self.context.kept_file,
        );
        enabled.keep = has_kept_file && !state.kept;
        self.keep_tip = keep_tip(state, &self.context.kept_file);
        enabled.revert = has_kept_file;
        self.revert_tip = if has_kept_file {
            String::from(
                "Read the kept file again, in the place of the session's customisation (CUSTOMISE \
                 REVERT)",
            )
        } else {
            no_kept_file()
        };
        // A button whose editor nobody handed over has nothing to open.
        enabled.edit_linework = self.context.open_linework.is_some();
        enabled.open_codes = self.context.open_code_manager.is_some();
        enabled.open_symbols = self.context.open_symbol_library.is_some();
    }

    /// The chooser as the Document and the built-ins say.
    fn show_built_ins(&mut self, host: &dyn Host) {
        if !self.alive() {
            return;
        }
        let document = host.document();
        let given;
        let entries: &[BuiltInEntry] = match &self.context.built_ins {
            Some(offered) => {
                given = offered();
                &given
            }
            None => built_ins(),
        };
        let choices = built_in_choices(document, entries);
        self.active_built_in = active_built_in_words(&choices, document.customisation_state());

        // The chooser is rebuilt from the built-ins and keeps the person's choice by name; with
        // none made yet it stands on the built-in the session is, else on the default, else on the
        // first.
        let before = self
            .built_in
            .index
            .and_then(|at| self.built_in_names.get(at))
            .cloned()
            .unwrap_or_default();
        self.built_in_items = choices.iter().map(built_in_item_text).collect();
        self.built_in_names = choices.iter().map(|choice| choice.name.clone()).collect();
        self.built_in_tips = choices.iter().map(built_in_tip).collect();
        let stand_on = choices
            .iter()
            .position(|choice| !before.is_empty() && choice.name == before)
            .or_else(|| choices.iter().rposition(|choice| choice.is_active))
            .or_else(|| choices.iter().rposition(|choice| choice.is_default));
        self.built_in.index = stand_on.or(if choices.is_empty() { None } else { Some(0) });

        // Worth a row only when there is something to choose between.
        self.built_in_row_shown = choices.len() >= 2;
        self.built_in_choices = choices;
        self.show_built_in_use(host);
    }

    /// Use as the chooser's choice leaves it: enabled, and what its tip says.
    fn show_built_in_use(&mut self, host: &dyn Host) {
        let Some(choice) = self
            .built_in
            .index
            .and_then(|at| self.built_in_choices.get(at))
        else {
            self.enabled.built_in_use = false;
            self.built_in_use_tip =
                String::from("This program has no built-in customisation to use.");
            return;
        };
        self.enabled.built_in_use =
            !choice.is_active && choice.problem.is_empty() && self.context.run.is_some();
        self.built_in_use_tip = if choice.is_active {
            String::from("The session is this built-in customisation already.")
        } else if !choice.problem.is_empty() {
            choice.problem.clone()
        } else {
            // The tip follows the Document, as Reset's does: a dialog whose Document has gone
            // says nothing of what a press would keep.
            let kept = self.alive() && host.document().customisation_state().kept;
            use_tip(kept, choice.is_default, &self.context.kept_file)
        };
    }

    fn use_built_in(&mut self, host: &mut dyn Host) {
        if !self.may_replace_rules(host, "Use Built-in") {
            return;
        }
        let name = self
            .built_in
            .index
            .and_then(|at| self.built_in_names.get(at))
            .cloned()
            .unwrap_or_default();
        if name.is_empty() {
            self.refuse(host, "Use Built-in: no built-in customisation is chosen.");
            return;
        }
        match command_word(&name, "Use Built-in") {
            Ok(word) => self.run(
                host,
                &format!("CUSTOMISE BUILTIN {word}"),
                Then::KeepWhatWasKept,
            ),
            Err(error) => self.refuse(host, &error.message),
        }
    }

    /// The two boxes as the Document has them.
    fn show_automation(&mut self, host: &dyn Host) {
        if !self.alive() {
            return;
        }
        let automation = &host.document().customisation_state().automation;
        self.auto_codes = automation.codes_on_survey_import;
        self.auto_linework = automation.linework_on_survey_import;
    }

    /// Every control that acts on the Document off, and the reason in the status. What the page
    /// last showed stays to be read - the notice too - and nothing that would act can be pressed.
    fn show_closed(&mut self) {
        self.enabled = Enabled::all(false);
        let message = if self.context.reads_drawing {
            "The drawing this dialog worked on has closed: nothing here acts on anything any more."
        } else {
            "Settings was opened on no drawing: nothing here acts on anything."
        };
        self.say(message, true);
    }

    // ---- saying and running ---------------------------------------------------------------------

    /// The status box.
    fn say(&mut self, message: &str, is_error: bool) {
        self.status = String::from(message);
        self.status_error = is_error;
    }

    /// The status box, and the log too: what the dialog refused before any line was built. The
    /// lines it runs are logged by the executor.
    fn refuse(&mut self, host: &mut dyn Host, message: &str) {
        self.say(message, true);
        host.log(message, true);
    }

    /// `line` through the executor; None when the dialog was given none.
    fn execute(&mut self, host: &mut dyn Host, line: &str) -> Option<VerbOutcome> {
        // Taken out for the call, so that the dialog is whole while the window runs the line.
        let mut run = self.context.run.take()?;
        let outcome = run(host, line);
        self.context.run = Some(run);
        Some(outcome)
    }

    fn no_command_line(line: &str) -> String {
        format!("Nothing here can run {line}: Settings was given no command line.")
    }

    /// Runs `line` through the executor and shows what it answered, and after it what `then` says.
    fn run(&mut self, host: &mut dyn Host, line: &str, then: Then) {
        if !self.alive() {
            self.show_closed();
            return;
        }
        let was_kept = host.document().customisation_state().kept;
        let Some(outcome) = self.execute(host, line) else {
            self.refuse(host, &Self::no_command_line(line));
            return;
        };
        if !self.alive() {
            self.show_closed();
            return;
        }
        let mut shown = vec![answered(line, &outcome)];
        let mut refused = !outcome.ok;
        // What is edited in Settings is kept: a session that was the kept one before this line,
        // and is not after it, is written to the kept file by the verb that does that. Asked of
        // the Document, not of the line: one that was refused, or that changed nothing, left the
        // session kept.
        if outcome.ok
            && then == Then::KeepWhatWasKept
            && was_kept
            && !self.context.kept_file.is_empty()
            && !host.document().customisation_state().kept
        {
            let keep = "CUSTOMISE KEEP";
            // The executor is there: it has just run a line.
            let kept = self.execute(host, keep).unwrap_or_default();
            if !self.alive() {
                self.show_closed();
                return;
            }
            shown.push(answered(keep, &kept));
            refused = !kept.ok;
        }
        self.say(&shown.join("\n"), refused);
        // The line's own changes are seen before the dialog goes on.
        self.look(&*host);
    }

    /// Whether a line that replaces the session's rules may run; says why not.
    fn may_replace_rules(&mut self, host: &mut dyn Host, what: &str) -> bool {
        if !self.alive() {
            self.show_closed();
            return false;
        }
        // In the words the window refuses to close over the same edits with. The manager keeps its
        // buffer when the drawing's rules change under it, and its Apply then installs that
        // buffer whole.
        if self
            .context
            .code_manager_dirty
            .as_ref()
            .is_some_and(|dirty| dirty())
        {
            self.refuse(
                host,
                &format!(
                    "{what} was not run: the Survey Code Manager has rule edits that are not on \
                     the drawing, and its Apply would then put the rules it holds back over what \
                     {what} brought. Apply or Revert them first."
                ),
            );
            return false;
        }
        true
    }

    fn import_file(&mut self, host: &mut dyn Host) {
        if !self.may_replace_rules(host, "Import") {
            return;
        }
        match typed_customisation_file_word(&self.import_path) {
            Ok(file) => {
                let verb = if self.import_replace {
                    "CUSTOMISE REPLACE "
                } else {
                    "CUSTOMISE "
                };
                self.run(host, &format!("{verb}{file}"), Then::KeepWhatWasKept);
            }
            Err(error) => self.refuse(host, &format!("Import: {}", error.message)),
        }
    }

    fn export_file(&mut self, host: &mut dyn Host) {
        match typed_customisation_file_word(&self.export_path) {
            // Never followed by KEEP: an export changes nothing of the session.
            Ok(file) => self.run(host, &format!("CUSTOMISE EXPORT {file}"), Then::Nothing),
            Err(error) => self.refuse(host, &format!("Export: {}", error.message)),
        }
    }

    /// The system's file dialog, or the one the context gave in its place.
    fn choose_file(&mut self, ask: &FileDialogAsk) -> String {
        match self.context.choose_file.as_mut() {
            Some(choose) => choose(ask),
            None if ask.save => get_save_file_name(&ask.title, &ask.start, &ask.filter),
            None => get_open_file_name(&ask.title, &ask.start, &ask.filter),
        }
    }

    fn browse_import(&mut self, host: &mut dyn Host) {
        if host.headless() {
            self.refuse(
                host,
                "A headless session opens no file dialog: type CUSTOMISE [REPLACE] <file> \
                 instead, or fill settingsImportPath.",
            );
            return;
        }
        // Opened where the field already points, read as Import reads it.
        let chosen = self.choose_file(&FileDialogAsk {
            save: false,
            title: String::from("Import a Katana Customisation"),
            start: typed_file_path(&self.import_path),
            filter: format!("{};;All files (*)", customisation_file_filter()),
        });
        if !chosen.is_empty() {
            self.import_path = from_native_separators(&chosen);
        }
    }

    fn browse_export(&mut self, host: &mut dyn Host) {
        if host.headless() {
            self.refuse(
                host,
                "A headless session opens no file dialog: type CUSTOMISE EXPORT <file> instead, \
                 or fill settingsExportPath.",
            );
            return;
        }
        let mut chosen = self.choose_file(&FileDialogAsk {
            save: true,
            title: String::from("Export the Customisation"),
            start: typed_file_path(&self.export_path),
            filter: String::from(customisation_file_filter()),
        });
        if chosen.is_empty() {
            return;
        }
        // A name that does not end .json, in any letter case, is given a customisation file's
        // ending, so that the file is one Import's own filter shows; one that does is left as the
        // person wrote it.
        if !folded(&chosen).ends_with(".json") {
            chosen.push_str(".customisation.json");
        }
        self.export_path = from_native_separators(&chosen);
    }

    fn automation_toggled(&mut self, host: &mut dyn Host, key: &str, on: bool) {
        self.run(
            host,
            &format!("CUSTOMISE SET {key}={}", if on { "on" } else { "off" }),
            Then::KeepWhatWasKept,
        );
        // The box shows what the DOCUMENT has. A line that was refused, or that nothing ran, left
        // the Document as it was, and no notification comes to say so: the box goes back.
        self.show_automation(&*host);
    }

    // ---- the Updates page -----------------------------------------------------------------------

    /// What the Updates page shows and writes to, in the place of what the context gave; the page
    /// is filled again from it.
    pub fn set_updates(&mut self, updates: SettingsUpdates) {
        self.context.updates = updates;
        self.show_updates();
    }

    /// The box, the two labels and which buttons can be pressed, as the context's updates say now.
    fn show_updates(&mut self) {
        let updates = &self.context.updates;
        let lines = self.context.run.is_some();
        let page = &mut self.updates;

        page.note.clone_from(&updates.note);
        page.sends = update_sends_words(updates);

        // The box shows what the STATE has, as the automation boxes show the Document: a line
        // that was refused left it as it was, and the box goes back. With nobody to ask it is off
        // and disabled, never a guess.
        page.auto = updates
            .automatic
            .as_ref()
            .is_some_and(|automatic| automatic());
        page.auto_enabled = lines && updates.automatic.is_some();

        let store = updates.credentials.as_deref();
        page.token_state = update_token_words(store);
        page.token_enabled = store.is_some();
        page.token_save_enabled = store.is_some();
        page.token_save_tip = if store.is_some() {
            String::from(
                "Store the token typed here for the update checks. It is written to the \
                 credential store and to nothing else: no line is run, so the command log never \
                 holds it",
            )
        } else {
            format!(
                "This session has no credential store to save a token in; the environment \
                 variable {TOKEN_ENVIRONMENT_VARIABLE} gives a run one."
            )
        };
        page.token_clear_enabled = lines;

        // With nobody to say which version is skipped - nobody to ask, or a state file that does
        // not read - the page says nothing of it, the button stays on and the verb answers; with
        // none skipped it has nothing to do.
        let skipped = updates
            .skipped_version
            .as_ref()
            .and_then(|skipped_version| skipped_version());
        page.skipped = skipped
            .as_deref()
            .map(update_skipped_words)
            .unwrap_or_default();
        page.unskip_enabled = lines && skipped.as_ref().is_none_or(|version| !version.is_empty());

        // The same of a version that was put back: on while nobody says whether one was, and off
        // when none was.
        let put_back = updates.put_back.as_ref().and_then(|put_back| put_back());
        page.blocked = put_back
            .map(|put_back| String::from(update_put_back_words(put_back)))
            .unwrap_or_default();
        page.unblock_enabled = lines && put_back.unwrap_or(true);
    }

    /// Runs an UPDATE line through the executor, shows what it answered and fills the page again.
    /// No Document is asked for: these lines are of the program, and run with no drawing and after
    /// one has closed.
    fn run_updates_line(&mut self, host: &mut dyn Host, line: &str) {
        match self.execute(host, line) {
            Some(outcome) => self.say(&answered(line, &outcome), !outcome.ok),
            None => self.refuse(host, &Self::no_command_line(line)),
        }
        self.show_updates();
    }

    /// Save: the typed token into the credential store, with no line.
    fn save_update_token(&mut self, host: &mut dyn Host) {
        let Some(store) = self.context.updates.credentials.clone() else {
            self.refuse(
                host,
                "Save: this session has no credential store to save a token in.",
            );
            return;
        };
        // The blanks round a pasted token are no part of it; anything else that makes it no token
        // is the store's to refuse.
        let typed = self.updates.token.trim();
        if typed.is_empty() {
            self.refuse(
                host,
                "Save: no access token is typed. Paste one, or press Clear to remove the stored \
                 one.",
            );
            return;
        }
        // NO LINE: a line is echoed in the command log, and a token must never be there. Nor is it
        // in anything said below - the store's refusal names the place and the rule, never the
        // value (katana_update's credential), and only its message is shown, never its context.
        if let Err(error) = store.write(typed.as_bytes()) {
            self.refuse(host, &format!("Save: {}", error.message));
            return;
        }
        self.updates.token.clear();
        self.say(
            &format!(
                "The access token was saved to the {}. It is not shown again.",
                store.r#where()
            ),
            false,
        );
        if let Some(token_changed) = &self.context.updates.token_changed {
            token_changed();
        }
        self.show_updates();
    }

    // ---- drawing --------------------------------------------------------------------------------

    /// One frame of the dialog, when it is shown.
    pub fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        self.look(&*host);
        if !self.visible {
            return;
        }
        let ctx = ui.ctx().clone();
        let mut frame = self.frame.clone();
        let mut lent = Lent {
            dialog: &mut *self,
            host: &mut *host,
        };
        let shown = frame.show(
            &ctx,
            &mut lent,
            |ui, lent| lent.dialog.body(ui, &mut *lent.host),
            |ui, lent| lent.dialog.footer(ui),
        );
        self.frame = frame;
        if shown.closed {
            self.close();
        }
    }

    fn body(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        ui.horizontal_top(|ui| {
            // The list chooses the page.
            ui.vertical(|ui| {
                ui.set_width(140.0);
                let rows: Vec<Vec<String>> = PAGES
                    .iter()
                    .map(|(title, _)| vec![String::from(*title)])
                    .collect();
                if controls::rows(ui, "settingsPages", &rows, &mut self.page).changed()
                    && self.page == Some(UPDATES_PAGE)
                {
                    // The Updates page shows per-user state that a typed line or the update
                    // dialog may have changed since: read again each time it is turned to.
                    self.show_updates();
                }
            });
            ui.add_space(layout::SECTION_GAP);
            let stack = ui.vertical(|ui| {
                ui.set_width(PAGE_WIDTH);
                let front = self.page.unwrap_or(0);
                for (at, (_, name)) in PAGES.iter().enumerate() {
                    let mut draw = |ui: &mut Ui| {
                        let page = ui.vertical(|ui| {
                            if at == UPDATES_PAGE {
                                self.updates_page(ui, &mut *host);
                            } else {
                                self.customisation_page(ui, &mut *host);
                            }
                        });
                        controls::custom(
                            ui,
                            &page.response,
                            ControlRecord::new(name, ControlKind::Other),
                        );
                    };
                    // A page that is not in front is drawn but not shown, so that a driver can
                    // fill a control on it, as it can on a hidden Qt page.
                    if at == front {
                        draw(ui);
                    } else {
                        offstage(ui, name, draw);
                    }
                }
            });
            let mut record = ControlRecord::new("settingsStack", ControlKind::Other);
            record.items = PAGES.iter().map(|(_, name)| String::from(*name)).collect();
            record.value = String::from(PAGES[self.page.unwrap_or(0).min(PAGES.len() - 1)].1);
            controls::custom(ui, &stack.response, record);
        });
    }

    fn customisation_page(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        // What is active.
        group(ui, "What is active", |ui| {
            dialog::form(ui, |form| {
                for (caption, name, text) in [
                    ("Name", "settingsActiveName", &self.name),
                    ("Origin", "settingsActiveOrigin", &self.origin),
                    ("Built-in", "settingsActiveBuiltIn", &self.active_built_in),
                    (
                        "Definitions",
                        "settingsActiveDefinitions",
                        &self.definitions,
                    ),
                    ("Symbols among them", "settingsActiveSymbols", &self.symbols),
                    ("Survey code rules", "settingsActiveRules", &self.rules),
                    ("Kept", "settingsActiveKept", &self.kept),
                ] {
                    form.row(caption, |ui| {
                        // On its label's line, as a field's text is.
                        dialog::text_value(ui, |ui| value_label(ui, name, text));
                    });
                }
            });
            // The next three are hidden until refresh finds something for them to show: a page
            // opened on no drawing has neither problems nor a notice.
            let problems = self.problems.clone();
            let colour = theme::error().to_egui();
            let draw_problems = |ui: &mut Ui| {
                controls::label_as(
                    ui,
                    "settingsActiveProblems",
                    &problems,
                    egui::Label::new(RichText::new(problems.as_str()).color(colour))
                        .wrap()
                        .selectable(true),
                );
            };
            if problems.is_empty() {
                hidden(ui, "settingsActiveProblems", draw_problems);
            } else {
                draw_problems(ui);
            }
            let mut toggled = false;
            let mut draw_toggle = |ui: &mut Ui| {
                toggled = controls::check(
                    ui,
                    "settingsShowNotice",
                    "Show Notice",
                    &mut self.show_notice,
                )
                .on_hover_text(SHOW_NOTICE_TIP)
                .changed();
            };
            if self.show_notice_shown {
                draw_toggle(ui);
            } else {
                hidden(ui, "settingsShowNotice", draw_toggle);
            }
            if toggled {
                self.notice_shown = self.show_notice && !self.notice.is_empty();
            }
            // Read, never typed into: what is drawn is a copy the frame throws away.
            let mut notice = self.notice.clone();
            let mut draw_notice = |ui: &mut Ui| {
                controls::text_with(ui, "settingsNotice", &mut notice, NOTICE_BOX);
            };
            if self.notice_shown {
                draw_notice(ui);
            } else {
                hidden(ui, "settingsNotice", draw_notice);
            }
        });

        // Import.
        // The file rows are labelled rows of the page's one label column, their fields filling
        // to the page's edge less their buttons: a path field 380 wide from the page's left edge
        // gave the page two field edges (173 and 310) and three right edges.
        group(ui, "Import", |ui| {
            dialog::labelled(ui, "File", |ui| {
                let width = dialog::beside_a_button(ui, "Browse...");
                ui.add_enabled_ui(self.enabled.import_path, |ui| {
                    controls::line_with(
                        ui,
                        "settingsImportPath",
                        &mut self.import_path,
                        LineOptions {
                            placeholder: IMPORT_PLACEHOLDER,
                            width: Some(width),
                            ..LineOptions::default()
                        },
                    );
                });
                ui.add_enabled_ui(self.enabled.import_browse, |ui| {
                    if controls::button(ui, "settingsImportBrowse", "Browse...").clicked() {
                        self.browse_import(&mut *host);
                    }
                });
            });
            dialog::labelled(ui, "", |ui| {
                ui.add_enabled_ui(self.enabled.import_replace, |ui| {
                    controls::check(
                        ui,
                        "settingsImportReplace",
                        "Replace instead of merging",
                        &mut self.import_replace,
                    )
                    .on_hover_text(IMPORT_REPLACE_TIP);
                });
                ui.add_enabled_ui(self.enabled.import, |ui| {
                    if controls::button(ui, "settingsImport", "Import")
                        .on_hover_text(IMPORT_TIP)
                        .clicked()
                    {
                        self.import_file(&mut *host);
                    }
                });
            });
        });

        // Export.
        group(ui, "Export", |ui| {
            dialog::labelled(ui, "File", |ui| {
                let width = dialog::beside_buttons(ui, &["Browse...", "Export"]);
                ui.add_enabled_ui(self.enabled.export_path, |ui| {
                    controls::line_with(
                        ui,
                        "settingsExportPath",
                        &mut self.export_path,
                        LineOptions {
                            placeholder: EXPORT_PLACEHOLDER,
                            width: Some(width),
                            ..LineOptions::default()
                        },
                    );
                });
                ui.add_enabled_ui(self.enabled.export_browse, |ui| {
                    if controls::button(ui, "settingsExportBrowse", "Browse...").clicked() {
                        self.browse_export(&mut *host);
                    }
                });
                ui.add_enabled_ui(self.enabled.export, |ui| {
                    if controls::button(ui, "settingsExport", "Export")
                        .on_hover_text(EXPORT_TIP)
                        .clicked()
                    {
                        self.export_file(&mut *host);
                    }
                });
            });
        });

        // The built-in and the kept file.
        group(ui, "Built-in and kept", |ui| {
            // The chooser of the built-ins: its row is hidden until refresh finds two to choose
            // between.
            let mut chosen = false;
            let mut used = false;
            let mut draw_row = |ui: &mut Ui| {
                // A scope round the row, whose rectangle is the row's record.
                let row = ui.scope(|ui| {
                    dialog::labelled(ui, "Built-in library", |ui| {
                        // The choice leaves its row room for Use after it.
                        let width = dialog::beside_a_button(ui, "Use");
                        dialog::fixed_fields(ui, width);
                        ui.add_enabled_ui(self.enabled.built_in_choice, |ui| {
                            let tip = self
                                .built_in
                                .index
                                .and_then(|at| self.built_in_tips.get(at))
                                .cloned()
                                .unwrap_or_default();
                            chosen = controls::choice(
                                ui,
                                "settingsBuiltIn",
                                &self.built_in_items,
                                &mut self.built_in,
                            )
                            .on_hover_text(tip)
                            .changed();
                        });
                        ui.add_enabled_ui(self.enabled.built_in_use, |ui| {
                            used = controls::button(ui, "settingsBuiltInUse", "Use")
                                .on_hover_text(self.built_in_use_tip.as_str())
                                .clicked();
                        });
                    });
                });
                controls::custom(
                    ui,
                    &row.response,
                    ControlRecord::new("settingsBuiltInRow", ControlKind::Other),
                );
            };
            if self.built_in_row_shown {
                draw_row(ui);
            } else {
                hidden(ui, "settingsBuiltInRow", draw_row);
            }
            // The button follows the chooser: it is for any built-in but the one the session
            // already is.
            if chosen {
                self.show_built_in_use(&*host);
            }
            if used {
                self.use_built_in(&mut *host);
            }
            // Under the fields, at their edge, as the Import row's box and button are.
            dialog::labelled(ui, "", |ui| {
                ui.add_enabled_ui(self.enabled.reset, |ui| {
                    if controls::button(ui, "settingsReset", "Reset to Built-in")
                        .on_hover_text(self.reset_tip.as_str())
                        .clicked()
                        && self.may_replace_rules(&mut *host, "Reset to Built-in")
                    {
                        self.run(&mut *host, "CUSTOMISE RESET", Then::KeepWhatWasKept);
                    }
                });
                ui.add_enabled_ui(self.enabled.keep, |ui| {
                    if controls::button(ui, "settingsKeep", "Keep")
                        .on_hover_text(self.keep_tip.as_str())
                        .clicked()
                    {
                        self.run(&mut *host, "CUSTOMISE KEEP", Then::Nothing);
                    }
                });
                ui.add_enabled_ui(self.enabled.revert, |ui| {
                    // Never followed by KEEP: it leaves the session as the kept file has it.
                    if controls::button(ui, "settingsRevert", "Revert to Kept")
                        .on_hover_text(self.revert_tip.as_str())
                        .clicked()
                        && self.may_replace_rules(&mut *host, "Revert to Kept")
                    {
                        self.run(&mut *host, "CUSTOMISE REVERT", Then::Nothing);
                    }
                });
            });
        });

        {
            // Automation. A box toggled, not clicked: one set by the headless driver's --fill has
            // been toggled as much as one a person clicked.
            group(ui, "When survey data comes in", |ui| {
                ui.add_enabled_ui(self.enabled.auto_codes, |ui| {
                    if controls::check(
                        ui,
                        "settingsAutoCodes",
                        "Apply survey codes",
                        &mut self.auto_codes,
                    )
                    .on_hover_text(AUTO_CODES_TIP)
                    .changed()
                    {
                        let on = self.auto_codes;
                        self.automation_toggled(&mut *host, "auto.codes", on);
                    }
                });
                ui.add_enabled_ui(self.enabled.auto_linework, |ui| {
                    if controls::check(
                        ui,
                        "settingsAutoLinework",
                        "Join coded points into lines",
                        &mut self.auto_linework,
                    )
                    .on_hover_text(AUTO_LINEWORK_TIP)
                    .changed()
                    {
                        let on = self.auto_linework;
                        self.automation_toggled(&mut *host, "auto.linework", on);
                    }
                });
            });

            // The editors.
            group(ui, "Editors", |ui| {
                ui.horizontal(|ui| {
                    ui.add_enabled_ui(self.enabled.open_codes, |ui| {
                        if controls::button(ui, "settingsOpenCodes", "Survey Codes...")
                            .on_hover_text(OPEN_CODES_TIP)
                            .clicked()
                            && let Some(open) = self.context.open_code_manager.as_mut()
                        {
                            open(&mut *host);
                        }
                    });
                    ui.add_enabled_ui(self.enabled.open_symbols, |ui| {
                        if controls::button(ui, "settingsOpenSymbols", "Symbol Library...")
                            .on_hover_text(OPEN_SYMBOLS_TIP)
                            .clicked()
                            && let Some(open) = self.context.open_symbol_library.as_mut()
                        {
                            open(&mut *host);
                        }
                    });
                });
            });
        }

        // The linework control codes: shown here, edited in ONE place.
        group(ui, "Linework control codes", |ui| {
            // The codes wrap above their button: the list is as long as the codes are many.
            ui.vertical(|ui| {
                value_label(ui, "settingsLinework", &self.linework).on_hover_text(LINEWORK_TIP);
                ui.add_enabled_ui(self.enabled.edit_linework, |ui| {
                    if controls::button(ui, "settingsEditLinework", "Edit Linework Codes...")
                        .on_hover_text(EDIT_LINEWORK_TIP)
                        .clicked()
                        && let Some(open) = self.context.open_linework.as_mut()
                    {
                        open(&mut *host);
                    }
                });
            });
        });
    }

    fn updates_page(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        // Hidden until show_updates finds something to say.
        let note = self.updates.note.clone();
        let draw_note = |ui: &mut Ui| {
            value_label(ui, "settingsUpdateNote", &note);
        };
        if note.is_empty() {
            hidden(ui, "settingsUpdateNote", draw_note);
        } else {
            draw_note(ui);
        }

        // Checking.
        group(ui, "Checking", |ui| {
            ui.add_enabled_ui(self.updates.auto_enabled, |ui| {
                if controls::check(
                    ui,
                    "settingsUpdateAuto",
                    "Check for updates automatically",
                    &mut self.updates.auto,
                )
                .on_hover_text(UPDATE_AUTO_TIP)
                .changed()
                {
                    let line = if self.updates.auto {
                        "UPDATE SET AUTO ON"
                    } else {
                        "UPDATE SET AUTO OFF"
                    };
                    self.run_updates_line(&mut *host, line);
                }
            });
            value_label(ui, "settingsUpdateSends", &self.updates.sends);
        });

        // The access token.
        group(ui, "Access token", |ui| {
            ui.add(egui::Label::new(TOKEN_WHY).wrap());
            ui.horizontal(|ui| {
                ui.add_enabled_ui(self.updates.token_enabled, |ui| {
                    controls::line_with(
                        ui,
                        "settingsUpdateToken",
                        &mut self.updates.token,
                        TOKEN_FIELD,
                    );
                });
                ui.add_enabled_ui(self.updates.token_save_enabled, |ui| {
                    if controls::button(ui, "settingsUpdateTokenSave", "Save")
                        .on_hover_text(self.updates.token_save_tip.as_str())
                        .clicked()
                    {
                        self.save_update_token(&mut *host);
                    }
                });
                ui.add_enabled_ui(self.updates.token_clear_enabled, |ui| {
                    if controls::button(ui, "settingsUpdateTokenClear", "Clear")
                        .on_hover_text(TOKEN_CLEAR_TIP)
                        .clicked()
                    {
                        // What was typed and not saved goes too: Clear leaves no token anywhere.
                        self.updates.token.clear();
                        self.run_updates_line(&mut *host, "UPDATE TOKEN CLEAR");
                    }
                });
            });
            value_label(ui, "settingsUpdateTokenState", &self.updates.token_state);
        });

        // The skipped version.
        group(ui, "Skipped version", |ui| {
            ui.horizontal(|ui| {
                value_label(ui, "settingsUpdateSkipped", &self.updates.skipped);
                ui.add_enabled_ui(self.updates.unskip_enabled, |ui| {
                    if controls::button(
                        ui,
                        "settingsUpdateUnskip",
                        "Offer the skipped version again",
                    )
                    .on_hover_text(UNSKIP_TIP)
                    .clicked()
                    {
                        self.run_updates_line(&mut *host, "UPDATE SKIP NONE");
                    }
                });
            });
        });

        // The version that was put back: under the skipped one, and built as it is - what the
        // state has, and the button that runs the line.
        group(ui, "Version that was put back", |ui| {
            ui.horizontal(|ui| {
                value_label(ui, "settingsUpdateBlocked", &self.updates.blocked);
                ui.add_enabled_ui(self.updates.unblock_enabled, |ui| {
                    if controls::button(
                        ui,
                        "settingsUpdateUnblock",
                        "Offer the version that was put back again",
                    )
                    .on_hover_text(UNBLOCK_TIP)
                    .clicked()
                    {
                        self.run_updates_line(&mut *host, "UPDATE BLOCKED CLEAR");
                    }
                });
            });
        });
    }

    fn footer(&mut self, ui: &mut Ui) {
        let placeholder = if self.status.is_empty() {
            STATUS_PLACEHOLDER
        } else {
            ""
        };
        // The one footer (shell::dialog::button_row): what the reply box is for, muted, at the
        // left until there is a reply; Close at the right.
        let close = dialog::button_row(
            ui,
            |ui| ui.weak(placeholder),
            |ui| controls::button(ui, "settingsClose", "Close").clicked(),
        )
        .1;
        // The reply under the row, read and never typed into: what is drawn is a copy the frame
        // throws away. Muted text, a refusal in the error colour, as wide as the dialog so the
        // reply of a load is read whole - and no room at all until there is one, where an empty
        // four-row box was a third of the footer.
        let mut status = self.status.clone();
        layout::reply_text(
            ui,
            "settingsStatus",
            &mut status,
            ControlKind::Text,
            self.status_error,
        );
        if close {
            self.close();
        }
    }
}

/// The Document the dialog reads, for a caller that has one and no host: the watcher's own type,
/// named here so that a reader finds what `reads_drawing` stands for.
pub type SettingsDocument = Document;

#[cfg(test)]
mod tests {
    use super::{count_of, with_built_in_undefined_as_a_count};

    // The header line of a load's report is "undefined names=<digits> in_built_in=<digits>" and
    // nothing else (settings_dialog.cpp's regular expression, anchored at both ends).
    #[test]
    fn only_the_whole_header_line_is_read_as_the_count() {
        assert_eq!(
            with_built_in_undefined_as_a_count("undefined names=2 in_built_in=1"),
            "2 names undefined, 1 of them as in the built-in"
        );
        for kept in [
            "undefined names=2 in_built_in=",
            "undefined names= in_built_in=1",
            "undefined names=2 in_built_in=1 more",
            " undefined names=2 in_built_in=1",
            "undefined names=two in_built_in=1",
        ] {
            assert_eq!(with_built_in_undefined_as_a_count(kept), kept);
        }
        assert_eq!(count_of("12"), Some(12));
        assert_eq!(count_of("-1"), None);
    }
}
