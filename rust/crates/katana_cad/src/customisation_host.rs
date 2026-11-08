//! Where a session's customisation comes from when it starts: the built-in one
//! and the user's kept one, and the one function that chooses between them
//! (include/katana/cad/customisation_host.hpp,
//! src/katana_cad/customisation/customisation_host.cpp,
//! src/katana_cad/customisation/builtin_customisation.cpp and builtin_table.hpp).
//!
//! A Document starts EMPTY and installs nothing by itself - a test, a tool and
//! a session given no host all draw plain lines, and say so. A front end that
//! wants the program's customisation hands over a HOST: the built-in (which a
//! build may not have) and the path of the file the user keeps their own in
//! (which a front end may not have: only the desktop has a per-user place).
//! start_customisation then does, once for every front end, what each would
//! otherwise do in its own words and with its own omissions.
//!
//! THE BUILT-INS are Katana customisation files compiled into the program
//! (CMake's KATANA_BUILTIN_CUSTOMISATION and KATANA_ALTERNATE_CUSTOMISATIONS,
//! which this crate's build.rs reads, and embeds with `include_bytes!` where
//! the C++ uses #embed). One of them is THE built-in, the DEFAULT: Katana
//! Standard, which a program starts with when nothing says otherwise. The
//! others are alternates, which a session may be given by name (CUSTOMISE
//! BUILTIN <name>) or a run may start with (the seam, below). The files are
//! tracked in the repository (resources/customisation), so a build made from
//! a clean checkout has them; a path that names no file is simply not offered,
//! and a build with none runs without one - a state the program runs in, not
//! a fault.
//!
//! THE SEAM. The environment variable KATANA_BUILTIN_CUSTOMISATION says what
//! "the built-in" is for one run, whatever was compiled in:
//!
//! ```text
//!   (unset, empty)  the default built-in, when there is one
//!   none            no built-in at all
//!   <name>          the compiled-in built-in of that name, in any letter case
//!                   (a value with no / or \ that names one)
//!   <file>          that Katana customisation file IS the built-in
//! ```
//!
//! Tried in that order: `none`, then a name, then a file. A file in the
//! working folder that is called like a built-in is given as ./name.
//!
//! It exists so that a test of a program is the same test on a machine whose
//! build has the built-in and on one whose build has not: the suite sets
//! `none` for programs that should start with nothing and names a small
//! committed fixture where the built-in path itself is under test. It is read
//! in ONE place, built_in_customisation(); what it then means is
//! built_in_customisation_for's to say, so that the rule itself is tested the
//! same on both machines.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use katana_core::error::make_error;
use katana_core::path_text::{environment_variable, path_from_utf8, path_to_utf8, read_file_bytes};
use katana_core::text::{equals_ignoring_case, lowered};
use katana_core::text_encoding::utf8_or_windows1252;
use katana_core::{Error, ErrorCode, Result, Status};
use katana_entity::customisation::{customisation_digest, customisation_from_json};
use katana_entity::{Customisation, CustomisationBase};

use crate::customisation_state::CustomisationOrigin;
use crate::document::Document;
use crate::style_catalogue::symbol_definition_count;

/// The Katana customisation files this build compiled into the program: what
/// build.rs writes into one generated file (the C++'s generated
/// builtin_customisation_table.cpp) and the functions below read. Internal to
/// the crate - the public view of them is built_ins().
mod builtin_table {
    /// One file the build embedded.
    #[allow(
        dead_code,
        reason = "A build whose configuration names no file constructs none."
    )]
    pub(super) struct CompiledInFile {
        /// The file's name without its directory, which is what an entry of
        /// built_ins() gives as its `source`.
        pub(super) file: &'static str,
        /// Its bytes, as they were on disk when the program was built. Never
        /// empty: an empty file stands for itself as one zero byte, which the
        /// reader refuses in so many words.
        pub(super) bytes: &'static [u8],
    }

    // COMPILED_IN_FILES: every file the build embedded, in the order it was
    // given - the DEFAULT built-in first when COMPILED_IN_HAS_DEFAULT, then the
    // alternates; empty in a build that names no file. COMPILED_IN_HAS_DEFAULT:
    // whether the first of them is the default built-in. False when only
    // alternates exist, and then a program has no built-in of its own - an
    // alternate is offered by name and never started with.
    include!(concat!(env!("OUT_DIR"), "/builtin_table.rs"));
}

/// A Katana customisation file, read: the customisation, and the digest of the
/// bytes it was read from (customisation_digest) - what tells one edition of a
/// name from another.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CustomisationFile {
    pub customisation: Customisation,
    pub digest: String,
}

fn path_text(path: &Path) -> String {
    utf8_or_windows1252(path_to_utf8(path))
}

/// Reads the file at `path` (customisation_from_json has what a file may be
/// refused for). NotFound when it cannot be opened, FileImportFailure when it
/// cannot be read; a refusal of its contents keeps its own code and message,
/// with the path put in front of its context.
pub fn read_customisation_file(path: &Path) -> Result<CustomisationFile> {
    let bytes = read_file_bytes(path)?;
    match customisation_from_json(&bytes) {
        Ok(customisation) => Ok(CustomisationFile {
            customisation,
            digest: customisation_digest(&bytes),
        }),
        Err(refusal) => {
            // The reader says which entry and which member; which FILE is ours
            // to add, and goes first so that a list of several reads as a list.
            let context = if refusal.context.is_empty() {
                path_text(path)
            } else {
                format!("{}: {}", path_text(path), refusal.context)
            };
            Err(make_error(refusal.code, refusal.message, context))
        }
    }
}

/// A built-in customisation as a host holds it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BuiltInCustomisation {
    /// None: there is none. Shared rather than copied: it is eight hundred
    /// definitions, and a host, a reset and a comparison all read the same one.
    pub customisation: Option<Arc<Customisation>>,
    /// The digest of the bytes it was read from; empty when there is none.
    pub digest: String,
    /// Why there is none although there should be one: the compiled-in bytes,
    /// or the file the seam names, are not a customisation. Empty when there
    /// is one, and when none was asked for - an absent built-in is not a
    /// problem.
    pub problem: String,
}

/// One built-in a program offers.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BuiltInEntry {
    /// The customisation's own name, which is what CUSTOMISE BUILTIN <name> and
    /// the seam take, in any letter case. For a file that did not read, the
    /// file's name.
    pub name: String,
    /// The customisation's own description; empty when it has none.
    pub description: String,
    /// The file the build compiled it in from, by its name alone. Empty for a
    /// built-in that was not compiled in: a file the seam named, a test's
    /// stand-in.
    pub source: String,
    /// As a host holds it; `problem` says why `customisation` is None.
    pub built_in: BuiltInCustomisation,
}

/// A Katana customisation file as text: its name without a folder, and its
/// bytes. What a build compiles in, per file, and the input of built_ins_from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BuiltInFile<'a> {
    pub file: &'a str,
    pub bytes: &'a [u8],
}

// A failure is reported, never thrown: a program whose built-in is damaged
// still starts, and says so.
fn parse_compiled_in(compiled: &BuiltInFile<'_>) -> BuiltInCustomisation {
    match customisation_from_json(compiled.bytes) {
        Ok(customisation) => BuiltInCustomisation {
            customisation: Some(Arc::new(customisation)),
            digest: customisation_digest(compiled.bytes),
            problem: String::new(),
        },
        Err(error) => BuiltInCustomisation {
            problem: format!(
                "the customisation compiled into this program ({}) is not read: {}",
                compiled.file,
                error.describe()
            ),
            ..BuiltInCustomisation::default()
        },
    }
}

// What the build compiled in, as the public type.
fn compiled_in_as_files() -> Vec<BuiltInFile<'static>> {
    builtin_table::COMPILED_IN_FILES
        .iter()
        .map(|compiled| BuiltInFile {
            file: compiled.file,
            bytes: compiled.bytes,
        })
        .collect()
}

// A value the seam may use as a built-in's name: it names no folder, so it
// cannot be a path to a file in one. (A file in the working folder that is
// called like a built-in is given as ./name, as a file called like a keyword
// is in the verbs.)
fn is_bare_word(seam: &[u8]) -> bool {
    !seam.contains(&b'/') && !seam.contains(&b'\\')
}

// "\"A\", \"B\"": the names of `named`, for a sentence.
fn names_of(named: &[BuiltInEntry]) -> String {
    let mut text = String::new();
    for entry in named {
        text.push_str(if text.is_empty() { "\"" } else { ", \"" });
        text.push_str(&entry.name);
        text.push('"');
    }
    text
}

/// The DEFAULT built-in compiled into this program, parsed once, at the first
/// call: KATANA_BUILTIN_CUSTOMISATION's file, and alone - the alternates are
/// not parsed for it, so a start costs the default's bytes only. None, with no
/// problem, in a build that has none. One that does not parse is reported in
/// `problem` and never thrown: a program whose built-in is damaged still
/// starts, and says so.
pub fn compiled_in_customisation() -> &'static BuiltInCustomisation {
    // Parsed at the first call and never again: it is some hundreds of
    // definitions, and a session that never asks pays nothing. The default
    // ALONE: an alternate is parsed by built_ins(), when a list or a name asks.
    static COMPILED_IN: OnceLock<BuiltInCustomisation> = OnceLock::new();
    COMPILED_IN.get_or_init(|| {
        let files = builtin_table::COMPILED_IN_FILES;
        match files.first() {
            Some(first) if builtin_table::COMPILED_IN_HAS_DEFAULT => {
                parse_compiled_in(&BuiltInFile {
                    file: first.file,
                    bytes: first.bytes,
                })
            }
            _ => BuiltInCustomisation::default(),
        }
    })
}

/// The entry of `entries` called `name`, letter case ignored; None when there
/// is none.
pub fn find_built_in<'a>(entries: &'a [BuiltInEntry], name: &str) -> Option<&'a BuiltInEntry> {
    find_built_in_named(entries, name.as_bytes())
}

// find_built_in of a name that may be narrow bytes: the seam's.
fn find_built_in_named<'a>(entries: &'a [BuiltInEntry], name: &[u8]) -> Option<&'a BuiltInEntry> {
    entries
        .iter()
        .find(|entry| equals_ignoring_case(entry.name.as_bytes(), name))
}

/// For a project drawn with customisations the session does not have
/// (CustomisationState::missing_at_open): the words that tell a person which
/// of them are built-ins this program offers, and what makes one the session -
/// `CUSTOMISE BUILTIN NSW makes the built-in "NSW" the session`, one clause for
/// each, joined with "; ". A project saved while an alternate was the session
/// records it by name, and a program that starts with another does not have it
/// loaded; the name is no file to load, so without these words nothing says
/// that it is here. Empty when none of `missing` is a usable entry of
/// `entries` (letter case ignored, spelled as the entry spells it).
pub fn built_in_advice_for(missing: &[String], entries: &[BuiltInEntry]) -> String {
    let mut advice = String::new();
    for name in missing {
        let Some(entry) = find_built_in(entries, name) else {
            continue;
        };
        if entry.built_in.customisation.is_none() {
            continue;
        }
        if !advice.is_empty() {
            advice.push_str("; ");
        }
        advice.push_str(&format!(
            "CUSTOMISE BUILTIN {} makes the built-in \"{}\" the session",
            entry.name, entry.name
        ));
    }
    advice
}

/// built_ins() for the files it is given, apart from the build that gave
/// them, so that its two rules are tested whatever this build compiled in: a
/// file that does not parse is listed under its file name with the reason in
/// `built_in.problem`, and a later file with an earlier one's name (letter
/// case ignored) is not offered. `first_parsed`, when given, is the first
/// file's built-in already parsed (compiled_in_customisation(), which is
/// shared, not parsed twice); it is taken as it is. (The C++ defaults it to
/// null.)
pub fn built_ins_from(
    files: &[BuiltInFile<'_>],
    first_parsed: Option<&BuiltInCustomisation>,
) -> Vec<BuiltInEntry> {
    let mut list: Vec<BuiltInEntry> = Vec::new();
    for (at, file) in files.iter().enumerate() {
        let built_in = match first_parsed {
            Some(parsed) if at == 0 => parsed.clone(),
            _ => parse_compiled_in(file),
        };
        let (name, description) = match &built_in.customisation {
            Some(customisation) => (
                customisation.name.clone(),
                customisation.description.clone(),
            ),
            None => (String::from(file.file), String::new()),
        };
        let entry = BuiltInEntry {
            name,
            description,
            source: String::from(file.file),
            built_in,
        };
        // One name, one entry: a second file of a name is not offered.
        if find_built_in(&list, &entry.name).is_none() {
            list.push(entry);
        }
    }
    list
}

/// EVERY built-in compiled into this program, parsed once, at the first call:
/// the default first when the build has one (it IS compiled_in_customisation(),
/// shared, not parsed twice), then the alternates in the order the build gave
/// them. A file that does not parse is listed under its file name with the
/// reason in `built_in.problem`; a later file whose customisation has an
/// earlier one's name (letter case ignored) is not offered, so that a name
/// picks one. Empty in a build that has none. Never fails.
pub fn built_ins() -> &'static [BuiltInEntry] {
    static ENTRIES: OnceLock<Vec<BuiltInEntry>> = OnceLock::new();
    ENTRIES.get_or_init(|| {
        // The default is the one parse compiled_in_customisation() made.
        let first = if builtin_table::COMPILED_IN_HAS_DEFAULT {
            Some(compiled_in_customisation())
        } else {
            None
        };
        built_ins_from(&compiled_in_as_files(), first)
    })
}

/// The built-in of THIS RUN: what the seam says, else the default -
/// built_in_customisation_for of the environment variable's value,
/// compiled_in_customisation() and, where the value is a bare word,
/// built_ins(). THIS, and not compiled_in_customisation(), is what a front end
/// asks for the host it hands over: once, at start-up. The variable is read at
/// each call. Never fails.
pub fn built_in_customisation() -> BuiltInCustomisation {
    // THE one place the seam is read. What it means is the function below's.
    // Through core, never std::env::var: on Windows the C++'s getenv gave the
    // ANSI code page's bytes, in which a file named outside the code page is a
    // file named with '?' (core's path_text).
    let seam = environment_variable(Some(BUILT_IN_CUSTOMISATION_VARIABLE));
    // Every alternate is parsed for a name to be found among them, which a
    // start that names a file, or none, or nothing, has no use for: it would
    // pay for megabytes it never reads.
    let may_be_a_name = !seam.is_empty() && lowered(&seam) != b"none" && is_bare_word(&seam);
    built_in_customisation_for(
        seam,
        compiled_in_customisation(),
        if may_be_a_name { built_ins() } else { &[] },
    )
}

/// The rule of the seam, apart from the things it is a rule about, so that it
/// can be tested whatever this build compiled in and whatever the environment
/// holds. `seam` is the variable's value - empty when it is not set -
/// `compiled_in` the default the program has, and `named` the built-ins it
/// offers by name (the C++ defaults it to none):
///
/// ```text
///   (empty)   `compiled_in`, as it is - its problem too, when it has one
///   none      no built-in and no problem, in any letter case
///   <name>    a value with no '/' or '\' that is the name of an entry of
///             `named`, letter case ignored: that entry's built-in, as it is
///   <file>    that Katana customisation file, read (read_customisation_file).
///             One that does not read gives none and the reason, naming the
///             variable and the file - never `compiled_in` in its place, which
///             would pass a test that named a fixture and got something else.
///             For a bare word, the reason also lists the names `named` has,
///             because a mistyped name is the likely cause.
/// ```
///
/// The file is named as text: UTF-8, or the narrow bytes an environment gave
/// (core's path_from_utf8, which takes both and fails on neither), which is
/// why `seam` is bytes. Where the reason repeats the seam, bytes that are not
/// UTF-8 are read as Windows-1252, core's one conversion for a message.
pub fn built_in_customisation_for(
    seam: impl AsRef<[u8]>,
    compiled_in: &BuiltInCustomisation,
    named: &[BuiltInEntry],
) -> BuiltInCustomisation {
    let seam = seam.as_ref();
    if seam.is_empty() {
        return compiled_in.clone();
    }
    if lowered(seam) == b"none" {
        return BuiltInCustomisation::default();
    }
    let bare_word = is_bare_word(seam);
    if bare_word && let Some(entry) = find_built_in_named(named, seam) {
        return entry.built_in.clone();
    }
    // As text, through core's one conversion: on Windows the seam's value
    // arrives as UTF-8, but elsewhere, and from any other caller, the text may
    // be narrow bytes that are not.
    let path = path_from_utf8(seam);
    match read_customisation_file(&path) {
        Ok(file) => BuiltInCustomisation {
            customisation: Some(Arc::new(file.customisation)),
            digest: file.digest,
            problem: String::new(),
        },
        Err(error) => {
            let mut problem = format!(
                "the file {BUILT_IN_CUSTOMISATION_VARIABLE} names as the built-in customisation \
                 is not read: {}",
                error.describe()
            );
            // A word that is no built-in's name is most likely a name mistyped,
            // and the file it was read as says only that it is not there.
            if bare_word && !named.is_empty() {
                problem.push_str(&format!(
                    "; it is no built-in customisation of this program either (it has {}; a file \
                     of that name in this folder is ./{})",
                    names_of(named),
                    utf8_or_windows1252(seam)
                ));
            }
            BuiltInCustomisation {
                problem,
                ..BuiltInCustomisation::default()
            }
        }
    }
}

/// The name of the environment variable the seam is.
pub const BUILT_IN_CUSTOMISATION_VARIABLE: &str = "KATANA_BUILTIN_CUSTOMISATION";

/// The name of the environment variable that names the kept file: the only way
/// a headless run, katana_cli or katana_mcp has one (a run that must be the
/// same every time reads no per-user place), and what overrides the desktop's
/// own. cad never reads it - a front end does, when it builds its host - but
/// the name is here, beside the seam's, because CUSTOMISE KEEP says it to a
/// session that was given no kept file.
pub const KEPT_CUSTOMISATION_VARIABLE: &str = "KATANA_CUSTOMISATION";

/// What a front end hands over.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CustomisationHost {
    /// The built-in of this run: what RESET gives and a start falls back to.
    pub built_in: BuiltInCustomisation,
    /// Where the user's own customisation is kept between runs. Empty: this
    /// front end keeps none. The file need not exist.
    pub kept_file: PathBuf,
    /// The built-ins this host offers by name (CUSTOMISE BUILTIN <name>).
    /// None: those compiled into the program, built_ins(), which is what every
    /// front end wants and none sets. A test sets it to stand in for the
    /// build, so that it is the same test on a machine whose build compiled in
    /// different files.
    pub built_ins: Option<Vec<BuiltInEntry>>,
}

/// The built-ins `host` offers by name: its own list when it gave one, else
/// those compiled into the program. In the build's order, the default first
/// when it has one. The built-in of the run (host.built_in) is one of them in
/// a real run, whether it is the default or the seam named another by name; a
/// seam that names a FILE makes a built-in that is none of them, which RESET
/// still gives and no list shows.
pub fn built_ins_of(host: &CustomisationHost) -> &[BuiltInEntry] {
    match &host.built_ins {
        Some(list) => list,
        None => built_ins(),
    }
}

/// One built-in as a person or an agent is shown it: what it holds, and how it
/// stands to this session. The one answer that CUSTOMISE BUILTIN, the JSON
/// report (customisation_json) and Settings' chooser all read, so that the
/// three cannot come to disagree.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BuiltInChoice {
    pub name: String,
    pub description: String,
    pub source: String,
    /// Why it is not usable, when it is not; the counts are then 0.
    pub problem: String,
    /// Linestyles and symbols together.
    pub definitions: usize,
    /// Distinct keys among the rules.
    pub codes: usize,
    pub rules: usize,
    /// The built-in of the run: the one RESET gives and the next start falls
    /// back to. (CustomisationState::built_in names it.)
    pub is_default: bool,
    /// The session IS this built-in as installed: its origin is the built-in
    /// and its name is this entry's. A kept or edited copy of it is not
    /// active; it is based on it (CustomisationState::based_on).
    pub is_active: bool,
}

/// `entries` as choices, in their order, against `document`'s session.
pub fn built_in_choices(document: &Document, entries: &[BuiltInEntry]) -> Vec<BuiltInChoice> {
    let state = document.customisation_state();
    let mut choices = Vec::with_capacity(entries.len());
    for entry in entries {
        let mut choice = BuiltInChoice {
            name: entry.name.clone(),
            description: entry.description.clone(),
            source: entry.source.clone(),
            problem: entry.built_in.problem.clone(),
            ..BuiltInChoice::default()
        };
        if let Some(customisation) = &entry.built_in.customisation {
            choice.definitions = customisation.library.size();
            choice.codes = customisation.map.keys().len();
            choice.rules = customisation.map.size();
        }
        choice.is_default = !state.built_in.is_empty() && entry.name == state.built_in;
        choice.is_active = state.origin == CustomisationOrigin::BuiltIn
            && entry.name == state.name
            && entry.built_in.customisation.is_some();
        choices.push(choice);
    }
    choices
}

/// Installs `built_in` as the session's customisation, origin BuiltIn, based
/// on ITSELF - based_on its own name and `built_in.digest`, whatever its file
/// says it was made from, and no based_on at all when the host gave no digest.
/// ONE place for that rule: a start with no kept file (start_customisation)
/// and CUSTOMISE RESET both install through it, and CUSTOMISE KEEP asks it
/// what "the session is the built-in" means. `kept`: whether the next start
/// would give this session (Document::install_customisation).
///
/// InvalidState when `built_in` holds none; otherwise whatever
/// Document::install_customisation refuses, the Document left as it was.
pub fn install_built_in_customisation(
    document: &mut Document,
    built_in: &BuiltInCustomisation,
    kept: bool,
) -> Status {
    let Some(customisation) = &built_in.customisation else {
        return Err(Error::new(
            ErrorCode::InvalidState,
            "there is no built-in customisation to install",
        ));
    };
    // The one copy of it an install makes: the Document owns what it is
    // drawn with, and an editor changes that in place.
    let mut copy = Customisation::clone(customisation);
    // Based on ITSELF, whatever its file says it was made from: a built-in
    // that was itself exported from a session names an earlier one, and a
    // copy kept from it would carry that digest - not this built-in's - and
    // be reported as made from another, at every start, though nothing had
    // changed. With no digest there is nothing to be based on by.
    copy.based_on = None;
    if !built_in.digest.is_empty() {
        copy.based_on = Some(CustomisationBase {
            name: copy.name.clone(),
            digest: built_in.digest.clone(),
        });
    }
    document.install_customisation(copy, CustomisationOrigin::BuiltIn, kept)
}

/// What start_customisation did, for the front end to say in its own words.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CustomisationStart {
    /// Kept, BuiltIn, or None when nothing was installed.
    pub installed: CustomisationOrigin,
    pub name: String,
    /// Linestyles and symbols together.
    pub definitions: usize,
    /// Those offered as symbols.
    pub symbols: usize,
    pub rules: usize,
    pub colours: usize,
    /// The kept customisation was made from another built-in than this program
    /// has, or another edition of it: it says what it is based on, and that is
    /// neither the run's built-in nor any other the program offers, in the
    /// edition it has (name and digest). A copy of an alternate the user chose
    /// and kept (CUSTOMISE BUILTIN <name>, then KEEP) is made from a built-in
    /// the program has, so it is not this. The kept customisation is installed
    /// all the same - it is the user's - and they may want to know that the
    /// built-in has moved on. False when the kept file does not say, and when
    /// there is no built-in to have moved on.
    pub kept_from_another_built_in: bool,
    /// What went wrong, in sentences: the built-in that did not parse, the
    /// kept file that did not read (the built-in then stands in for it).
    pub problems: Vec<String>,
}

// What is installed now, as the report gives it.
fn summarise(report: &mut CustomisationStart, document: &Document, origin: CustomisationOrigin) {
    report.installed = origin;
    report.name.clone_from(&document.customisation_state().name);
    report.definitions = document.style_library().size();
    report.symbols = symbol_definition_count(document);
    report.rules = document.survey_map().size();
    report.colours = document.customisation_state().colours.size();
}

// Whether a kept customisation that says it is `based_on` was made from
// something the program does not have. The run's own built-in is the common
// case and is judged first: by its name, and by its bytes (a host that could
// give no digest has no edition to differ from). Another built-in the program
// offers is not "another": the user chose it (CUSTOMISE BUILTIN, then KEEP),
// and a copy of it kept says so in the same two words. It is another when the
// program has no such built-in, or has another edition of it.
fn made_from_another_built_in(based_on: &CustomisationBase, host: &CustomisationHost) -> bool {
    let mut ours = &host.built_in;
    let own_name = host
        .built_in
        .customisation
        .as_ref()
        .map_or("", |customisation| customisation.name.as_str());
    // The list is looked through only when the name is not the run's: it may
    // be parsed for it, and a start that finds its own needs none of that.
    if based_on.name != own_name {
        let offered = built_ins_of(host);
        let Some(found) = offered
            .iter()
            .find(|entry| entry.name == based_on.name && entry.built_in.customisation.is_some())
        else {
            return true;
        };
        ours = &found.built_in;
    }
    !ours.digest.is_empty() && based_on.digest != ours.digest
}

// The choice start_customisation makes, and its report.
fn choose_and_install(document: &mut Document, host: &CustomisationHost) -> CustomisationStart {
    let mut report = CustomisationStart::default();
    let has_built_in = host.built_in.customisation.is_some();
    if !host.built_in.problem.is_empty() {
        report.problems.push(host.built_in.problem.clone());
    }
    document.set_built_in_customisation_name(
        host.built_in
            .customisation
            .as_ref()
            .map_or_else(String::new, |customisation| customisation.name.clone()),
    );

    // The user's own first: it is what they kept. A kept file that is not
    // there is the ordinary case - nobody has kept anything - and is no
    // problem; one that is there and does not read is said, and the built-in
    // stands in for it rather than the session starting with nothing.
    if !host.kept_file.as_os_str().is_empty() {
        // When it cannot be told, it is taken to be there, as the C++'s
        // `exists(path, unknown) || unknown`.
        let there = host.kept_file.try_exists().unwrap_or(true);
        if there {
            let fallback = if has_built_in {
                "the built-in customisation is used"
            } else {
                "no customisation is loaded"
            };
            match read_customisation_file(&host.kept_file) {
                Err(error) => report.problems.push(format!(
                    "the kept customisation is not read, so {fallback}: {}",
                    error.describe()
                )),
                Ok(kept) => {
                    let based_on = kept.customisation.based_on.clone();
                    match document.install_customisation(
                        kept.customisation,
                        CustomisationOrigin::Kept,
                        true,
                    ) {
                        Ok(()) => {
                            summarise(&mut report, document, CustomisationOrigin::Kept);
                            if let Some(based_on) = &based_on
                                && has_built_in
                            {
                                report.kept_from_another_built_in =
                                    made_from_another_built_in(based_on, host);
                            }
                            return report;
                        }
                        Err(error) => report.problems.push(format!(
                            "the kept customisation is not installed, so {fallback}: {}",
                            error.describe()
                        )),
                    }
                }
            }
        }
    }

    if has_built_in {
        if let Err(error) = install_built_in_customisation(document, &host.built_in, true) {
            report.problems.push(format!(
                "the built-in customisation is not installed: {}",
                error.describe()
            ));
            return report;
        }
        summarise(&mut report, document, CustomisationOrigin::BuiltIn);
    }
    report
}

/// Installs the customisation a session starts with: the kept file when it
/// exists and reads (origin Kept), else the built-in (origin BuiltIn), else
/// nothing, and the Document stays as it was. What it installs is `kept`: it
/// is what the next start would give, the linework codes and the automation
/// included (Document::install_customisation). The built-in is installed
/// based on ITSELF - based_on its own name and the digest of its own bytes,
/// whatever its file says it was made from - so that a copy kept from it later
/// can be told from one made against another edition. (A built-in that was
/// itself exported from a session names an earlier one; kept as that, every
/// copy made from it would be reported as made from another built-in, at
/// every start.)
///
/// It also tells the Document the built-in's name, which answers the names a
/// project recorded for the built-in's earlier files, and what this start
/// found - the report's `problems` and `kept_from_another_built_in`
/// (Document::set_customisation_start) - so that a report of the session
/// asked for later (customisation_json) still says what a front end said once.
pub fn start_customisation(
    document: &mut Document,
    host: &CustomisationHost,
) -> CustomisationStart {
    let report = choose_and_install(document, host);
    // Left on the Document as well as handed back. The front end says it
    // once, where its errors go; whoever asks what the session holds later -
    // CUSTOMISE JSON, a client of katana_mcp, which is never shown that
    // stream - is then still told that the customisation it kept is not the
    // one it got.
    document.set_customisation_start(report.problems.clone(), report.kept_from_another_built_in);
    report
}
