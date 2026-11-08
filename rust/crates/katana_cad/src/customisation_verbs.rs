//! CUSTOMISE: the session's customisation on the text command line
//! (docs/customisation.md, "The verbs"; include/katana/cad/customisation_verbs.hpp,
//! src/katana_cad/customisation/customisation_verbs.cpp). The interpreter's,
//! so the window's command line, katana_cli, katana_mcp and an AI agent run
//! ONE family; it was written twice, by hand, in the two front ends, because
//! the readers of the older files lived where cad could not see them, and the
//! two had come to differ (one refused a load with problems, the other
//! installed it).
//!
//! ```text
//!   CUSTOMISE                            what is loaded, as records
//!   CUSTOMISE JSON                       the same as one JSON object
//!   CUSTOMISE <file>...                  merge Katana customisation files in
//!   CUSTOMISE REPLACE <file>...          in the place of each kind they bring
//!   CUSTOMISE DEFINITIONS <file>...      merge their definitions and colours alone
//!   CUSTOMISE EXPORT <file> [CODES] [LINESTYLES] [SYMBOLS] [NAME <name>]
//!                                        [ONLY <definition>...]
//!   CUSTOMISE RESET                      the host's built-in customisation (the
//!                                        run's own: the default unless the seam says)
//!   CUSTOMISE BUILTIN                    the built-ins the host offers
//!   CUSTOMISE BUILTIN <name>             one of them, in the place of the session's
//!   CUSTOMISE KEEP                       write the session to the kept file
//!   CUSTOMISE REVERT                     read the kept file again
//!   CUSTOMISE REMOVE <definition>... [FORCE]
//!   CUSTOMISE REMOVE CODE <key>...
//!   CUSTOMISE SET <key>=<value>...       auto.codes, auto.linework, linework.*
//! ```
//!
//! A KEYWORD IS THE WHOLE FIRST WORD, in any case. The command line removes
//! quotes before a verb sees its words, so "a quoted word is a file" - the
//! rule the session's own parser kept - cannot be kept here; a file that is
//! literally called as a keyword is given with its directory (./json), as a
//! file called as a scope word is (scope_verbs). The interpreter hands over
//! the words after the CANONICAL verb, so CUSTOMIZE is the same family.
//!
//! THE FRAGMENT IS THE ONE DOOR FOR AN EDIT. Nothing here edits a rule or a
//! definition in place: a customisation file holding only what changes is
//! merged in (CUSTOMISE <file>), which replaces a definition by its name and a
//! key's rules by their section. REMOVE exists because a merge cannot delete,
//! and SET because the switches and the control codes are settings, not data.
//!
//! THE HOST is what a front end hands the interpreter - the built-in
//! customisation and the path of the kept file (customisation_host). A
//! session given none (a test's bare Document, a tool) has everything here but
//! RESET, BUILTIN, KEEP and REVERT, which are refused by name.
//!
//! Every reply is key=value records, one a line (record_value, scope_verbs), a
//! file written with '/' in its path; no reply to a line that succeeded holds
//! the word a script fails on.

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use katana_core::error::make_error;
use katana_core::path_text::{path_from_utf8, path_to_utf8, read_file_bytes};
use katana_core::text::equals_ignoring_case;
use katana_core::text_encoding::utf8_or_windows1252;
use katana_core::{Error, ErrorCode, Result, Status};
use katana_entity::customisation::{
    customisation_digest, customisation_from_json, customisation_to_json,
    validate_customisation_name,
};
use katana_entity::linework_codes::linework_code_members;
use katana_entity::{Customisation, CustomisationWriteOptions, SurveyMap};

use crate::customisation_host::{
    BuiltInCustomisation, CustomisationHost, KEPT_CUSTOMISATION_VARIABLE, built_in_choices,
    built_ins_of, compiled_in_customisation, find_built_in, install_built_in_customisation,
    read_customisation_file,
};
use crate::customisation_merge::{LoadMode, merge_customisation};
use crate::customisation_part::{customisation_part, definitions_of_file};
use crate::customisation_record::distinct_customisation_files;
use crate::customisation_report::{
    customisation_json, customisation_json_offering, customisation_state_record,
    customisation_summary, format_customisation_reply, undefined_rule_names,
};
use crate::customisation_state::CustomisationOrigin;
use crate::definition_users::definition_users;
use crate::document::Document;
use crate::scope_verbs::record_value;

/// What the family keeps between lines, beside the Document.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CustomisationVerbContext {
    /// None: no front end handed a host over.
    pub host: Option<CustomisationHost>,
    /// The kept file as this session last read or wrote it: the digest of its
    /// bytes (customisation_digest), or None when it was not there. CUSTOMISE
    /// KEEP refuses to write over a file that is another now: a second Katana
    /// kept its own there, and writing would lose it silently.
    pub kept_digest: Option<String>,
}

const LOAD_USAGE: &str = "CUSTOMISE [REPLACE] <file> [<file>...]";
const DEFINITIONS_USAGE: &str = "CUSTOMISE DEFINITIONS <file> [<file>...]";
const EXPORT_USAGE: &str = "CUSTOMISE EXPORT <file> [CODES] [LINESTYLES] [SYMBOLS] [NAME <name>] \
                            [ONLY <definition>...]";
const REMOVE_USAGE: &str =
    "CUSTOMISE REMOVE <definition>... [FORCE] | CUSTOMISE REMOVE CODE <key>...";
const SET_USAGE: &str = "CUSTOMISE SET <key>=<value>...; keys: auto.codes=on|off \
                         auto.linework=on|off \
                         linework.start|end|close|arcstart|arcend|join|rectangle=<word>";

// How many names of a list a reply prints before it only counts the rest: a
// Replace of the whole library removes hundreds.
const NAMES_LISTED: usize = 20;

fn usage(text: &str) -> Error {
    Error::new(ErrorCode::InvalidArgument, format!("usage: {text}"))
}

fn is(word: &str, name: &str) -> bool {
    equals_ignoring_case(word.as_bytes(), name.as_bytes())
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn path_text(path: &Path) -> String {
    utf8_or_windows1252(path_to_utf8(path))
}

// A path as a sentence says it: plainly, with '/' on every platform (the
// C++'s generic_u8string).
fn generic_text(path: &Path) -> String {
    let text = path_text(path);
    if cfg!(windows) {
        text.replace('\\', "/")
    } else {
        text
    }
}

// A path in a record: with '/', on every platform. A backslash in a record
// is an escape (record_value), so C:\site\codes.json would be written with
// every one doubled and read by nobody.
fn path_value(path: &Path) -> String {
    record_value(&generic_text(path))
}

// " definitions=<n> codes=<n> rules=<n>": what the session holds now.
fn counts_of(document: &Document) -> String {
    format!(
        " definitions={} codes={} rules={}",
        document.style_library().size(),
        document.survey_map().keys().len(),
        document.survey_map().size()
    )
}

// `names` once each, in the order first given.
fn distinct(names: &[String]) -> Vec<String> {
    let mut once: Vec<String> = Vec::new();
    for name in names {
        if !once.contains(name) {
            once.push(name.clone());
        }
    }
    once
}

// Up to NAMES_LISTED records naming one each, so that a reply says how many
// and shows enough of them to act on.
fn list_names(reply: &mut String, what: &str, key: &str, names: &[String]) {
    for name in names.iter().take(NAMES_LISTED) {
        reply.push_str(&format!("{what} {key}={}\n", record_value(name)));
    }
}

// The text std::error_code::message gives: the system's words, without the
// "(os error N)" Rust's error adds to them.
fn os_message(error: &io::Error) -> String {
    let text = format!("{error}");
    match (text.rfind(" (os error "), text.ends_with(')')) {
        (Some(at), true) => String::from(&text[..at]),
        _ => text,
    }
}

// ---- files -------------------------------------------------------------------------------

fn beside(file: &Path, suffix: &str) -> PathBuf {
    let mut other = file.as_os_str().to_owned();
    other.push(suffix);
    PathBuf::from(other)
}

// The digest of the bytes of `file`; None when it is not there, or cannot be
// read (a KEEP then finds it there and refuses for that).
fn digest_of_file(file: &Path) -> Option<String> {
    if file.as_os_str().is_empty() {
        return None;
    }
    read_file_bytes(file)
        .ok()
        .map(|bytes| customisation_digest(&bytes))
}

fn is_there(file: &Path) -> bool {
    // When it cannot be told, it is taken to be there: the read that follows
    // then says why, and nothing is written over what might be a file.
    file.try_exists().unwrap_or(true)
}

// A name beside `file` for the file that is written first and renamed into its
// place. It holds this process's id and a count, so that two Katanas keeping
// into one folder do not write one file, and it is never `<file>.tmp`, which
// is a name a person may have used for a file of their own: the temporary is
// removed or renamed away, and that name would be consumed.
fn temporary_beside(file: &Path) -> PathBuf {
    static MADE: AtomicU64 = AtomicU64::new(0);
    let count = MADE.fetch_add(1, Ordering::Relaxed) + 1;
    beside(file, &format!(".{}-{count}.tmp", std::process::id()))
}

// `bytes` as the whole of `file`: written to a file beside it, then renamed
// into its place, so that a write that fails part way leaves what was there.
fn put_file(file: &Path, bytes: &[u8], refusal: &str) -> Status {
    let failure = |code: Option<&io::Error>| {
        make_error(
            ErrorCode::FileExportFailure,
            refusal,
            match code {
                Some(error) => format!("{}: {}", path_text(file), os_message(error)),
                None => path_text(file),
            },
        )
    };
    let temporary = temporary_beside(file);
    if fs::write(&temporary, bytes).is_err() {
        let _ = fs::remove_file(&temporary);
        return Err(failure(None));
    }
    if let Err(error) = fs::rename(&temporary, file) {
        let _ = fs::remove_file(&temporary);
        return Err(failure(Some(&error)));
    }
    Ok(())
}

// The file a write replaces, as the caller read it: `bytes` are put in
// `beside` before the write, so that the earlier edition is kept.
struct Replaced<'a> {
    beside: PathBuf,
    bytes: &'a [u8],
}

// Writes `text` as the whole of `file`: to a file beside it first, which is
// then renamed into its place, so a write that fails part way leaves what was
// there. With `replaced`, the bytes of the file that is there are first put in
// the name it gives (an earlier file of that name is replaced) - WRITTEN from
// the bytes the caller read and judged, not copied from the file: a copy
// carries the file's attributes, and a read-only kept file made a read-only
// `.bak`, which the next write then could not replace. They are put before the
// file is replaced, and not by moving it, so that at no moment is there no
// `file`: a start between the two steps reads the one or the other, never the
// built-in by accident.
fn write_whole(file: &Path, text: &[u8], replaced: Option<&Replaced<'_>>) -> Status {
    if let Some(parent) = file.parent()
        && !parent.as_os_str().is_empty()
    {
        // A per-user folder that nothing has written to yet does not exist.
        // If it cannot be made, opening the file says so.
        let _ = fs::create_dir_all(parent);
    }
    let temporary = temporary_beside(file);
    if fs::write(&temporary, text).is_err() {
        let _ = fs::remove_file(&temporary);
        return Err(make_error(
            ErrorCode::FileExportFailure,
            "the file cannot be written",
            path_text(file),
        ));
    }
    if let Some(replaced) = replaced
        && let Err(kept) = put_file(
            &replaced.beside,
            replaced.bytes,
            "the file that is there could not be kept beside it, so nothing was written",
        )
    {
        let _ = fs::remove_file(&temporary);
        return Err(kept);
    }
    if let Err(error) = fs::rename(&temporary, file) {
        let _ = fs::remove_file(&temporary);
        return Err(make_error(
            ErrorCode::FileExportFailure,
            "the file cannot be written",
            format!("{}: {}", path_text(file), os_message(&error)),
        ));
    }
    Ok(())
}

// ---- the report ---------------------------------------------------------------------------

fn state_record(document: &Document) -> String {
    customisation_state_record(&customisation_summary(document))
}

fn trimmed_reply(mut text: String) -> String {
    while text.ends_with('\n') {
        text.pop();
    }
    text
}

// ---- loading ------------------------------------------------------------------------------

// The names the program's own built-in asks for and defines nowhere, which a
// load into a session that started from it reports again whatever the load
// was. Said apart from the load's own (undefined_reply), so that three names
// the built-in has always left are not mistaken for three the file just broke.
fn undefined_in_built_in(context: &CustomisationVerbContext) -> BTreeSet<String> {
    let Some(host) = &context.host else {
        return BTreeSet::new();
    };
    if host.built_in.customisation.is_none() {
        return BTreeSet::new();
    }
    let mut scratch = Document::default();
    if install_built_in_customisation(&mut scratch, &host.built_in, true).is_err() {
        return BTreeSet::new();
    }
    undefined_rule_names(&scratch).into_iter().collect()
}

// "undefined names=<n>", then a record for each of up to NAMES_LISTED. A name
// the built-in also leaves undefined says so (`in_built_in=yes`), and the
// count says how many do (`in_built_in=<k>`, only when k is not 0), so that a
// front end can show them once, as a count.
fn undefined_reply(undefined: &[String], own: &BTreeSet<String>) -> String {
    let in_built_in = undefined.iter().filter(|name| own.contains(*name)).count();
    let mut reply = format!("undefined names={}", undefined.len());
    if in_built_in != 0 {
        reply.push_str(&format!(" in_built_in={in_built_in}"));
    }
    reply.push('\n');
    for name in undefined.iter().take(NAMES_LISTED) {
        reply.push_str(&format!("undefined name={}", record_value(name)));
        reply.push_str(if own.contains(name) {
            " in_built_in=yes\n"
        } else {
            "\n"
        });
    }
    reply
}

// `definitions_only`: the files' definitions and colours alone are loaded
// (definitions_of_file), as the Symbol Library's Import Definitions loads
// them. (The C++ defaults it to false.)
fn load(
    document: &mut Document,
    context: &CustomisationVerbContext,
    files: &[String],
    mode: LoadMode,
    definitions_only: bool,
) -> Result<String> {
    if files.is_empty() {
        return Err(usage(if definitions_only {
            DEFINITIONS_USAGE
        } else {
            LOAD_USAGE
        }));
    }
    let named: Vec<PathBuf> = files.iter().map(path_from_utf8).collect();
    // A file named twice is read once: read twice, each of its rules would
    // sit in the map twice, since both copies are the load's.
    let once = distinct_customisation_files(&named);

    // Every file is read before anything is judged, so that a load is
    // refused once, with all that is wrong with it.
    let mut loaded: Vec<Customisation> = Vec::new();
    let mut unread: Vec<Error> = Vec::new();
    for file in &once.files {
        match read_customisation_file(file) {
            Ok(read) => loaded.push(read.customisation),
            Err(error) => unread.push(error),
        }
    }
    if unread.len() == 1 {
        // The reader's own words, whole: which file, and what it is not.
        return Err(unread.remove(0));
    }
    if let Some(first) = unread.first() {
        let mut message = format!(
            "{} of the {} files were not read, so nothing was loaded",
            unread.len(),
            once.files.len()
        );
        for refusal in &unread {
            message.push_str("\n  ");
            message.push_str(&refusal.describe());
        }
        return Err(Error::new(first.code, message));
    }

    let mut rules_left = 0usize;
    let mut linework_left = false;
    let mut automation_left = false;
    if definitions_only {
        let mut anything = false;
        for file in &mut loaded {
            let taken = definitions_of_file(std::mem::take(file));
            rules_left += taken.rules_left;
            linework_left = linework_left || taken.linework_left;
            automation_left = automation_left || taken.automation_left;
            anything = anything || !taken.taken.library.empty() || !taken.taken.colours.empty();
            *file = taken.taken;
        }
        if !anything {
            return Err(Error::new(
                ErrorCode::NotFound,
                format!(
                    "CUSTOMISE DEFINITIONS: the {} no definitions or colours to load, so nothing \
                     was loaded{}",
                    if once.files.len() == 1 {
                        "file holds"
                    } else {
                        "files hold"
                    },
                    if rules_left != 0 {
                        "; its survey code rules are loaded by CUSTOMISE <file>"
                    } else {
                        ""
                    }
                ),
            ));
        }
    }

    let merge = merge_customisation(&document.customisation(), &loaded, mode);
    if !merge.ok() {
        let mut message = format!(
            "nothing was loaded, for {} {}",
            merge.problems.len(),
            if merge.problems.len() == 1 {
                "reason"
            } else {
                "reasons"
            }
        );
        for problem in &merge.problems {
            message.push_str("\n  ");
            message.push_str(problem);
        }
        return Err(Error::new(ErrorCode::InvalidArgument, message));
    }
    // The merge judged the loads; the install judges the session's own parts
    // (a definition's source set by hand, a name it never read from a file).
    document.install_customisation(merge.merged, CustomisationOrigin::Loaded, false)?;

    let mut reply = String::new();
    for repeat in &once.repeats {
        reply.push_str(&format!("repeated file={}\n", path_value(repeat)));
    }
    for (one, file) in merge.loads.iter().zip(&once.files) {
        reply.push_str(&format!(
            "loaded file={} name={} definitions_added={} definitions_replaced={} codes_added={} \
             codes_replaced={} colours_added={} colours_replaced={} linework={} automation={}\n",
            path_value(file),
            record_value(&one.name),
            one.added_definitions.len(),
            one.replaced_definitions.len(),
            one.added_keys.len(),
            one.replaced_keys.len(),
            one.added_colours.len(),
            one.replaced_colours.len(),
            yes_no(one.linework),
            yes_no(one.automation)
        ));
    }
    if rules_left != 0 || linework_left || automation_left {
        reply.push_str(&format!(
            "left rules={rules_left} linework={} automation={}\n",
            yes_no(linework_left),
            yes_no(automation_left)
        ));
    }
    if !merge.removed_definitions.is_empty() || !merge.removed_keys.is_empty() {
        reply.push_str(&format!(
            "removed definitions={} codes={}\n",
            merge.removed_definitions.len(),
            merge.removed_keys.len()
        ));
        list_names(
            &mut reply,
            "removed",
            "definition",
            &merge.removed_definitions,
        );
        list_names(&mut reply, "removed", "code", &merge.removed_keys);
    }
    // Judged against everything now loaded: a file of codes may name what an
    // earlier load defined.
    let undefined = undefined_rule_names(document);
    if !undefined.is_empty() {
        reply.push_str(&undefined_reply(
            &undefined,
            &undefined_in_built_in(context),
        ));
    }
    reply.push_str(&state_record(document));
    Ok(reply)
}

// ---- export -------------------------------------------------------------------------------

// One of EXPORT's own words, in any case.
fn is_export_word(word: &str) -> bool {
    ["CODES", "LINESTYLES", "SYMBOLS", "NAME", "ONLY"]
        .iter()
        .any(|own| is(word, own))
}

// After ONLY every word is a definition's name - up to NAME, unless NAME was
// given before it, so that a definition of that name can be listed.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Reading {
    Options,
    Names,
    Done,
}

fn export_to(document: &Document, args: &[String]) -> Result<String> {
    let Some(first) = args.first() else {
        return Err(usage(EXPORT_USAGE));
    };
    // The first word is the file. One of EXPORT's own words there is a line
    // whose file was left out - CUSTOMISE EXPORT CODES wrote the whole session
    // into a file called CODES - and a file that IS so called is given with
    // its directory, as the family's keyword rule has it.
    if is_export_word(first) {
        // The usage first and the word last: a front end prints the context
        // in brackets after the message, where it would read as one more
        // option of the usage.
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!(
                "usage: {EXPORT_USAGE}: the file comes first, and this is one of EXPORT's own \
                 words; a file so called is given with its directory (./{first})"
            ),
            first.as_str(),
        ));
    }
    let file = path_from_utf8(first);
    let mut options = CustomisationWriteOptions::default();
    let mut name: Option<String> = None;
    let mut linestyles = false;
    let mut symbols = false;
    let mut codes = false;
    let mut only = false;
    let mut reading = Reading::Options;
    let mut at = 1;
    while at < args.len() {
        let word = &args[at];
        if reading == Reading::Done {
            return Err(usage(EXPORT_USAGE));
        }
        if is(word, "NAME") && name.is_none() {
            if at + 1 >= args.len() {
                return Err(usage(EXPORT_USAGE));
            }
            at += 1;
            name = Some(args[at].clone());
            if reading == Reading::Names {
                reading = Reading::Done;
            }
            at += 1;
            continue;
        }
        if reading == Reading::Names {
            options.only.push(word.clone());
        } else if is(word, "ONLY") {
            only = true;
            reading = Reading::Names;
        } else if is(word, "CODES") {
            codes = true;
        } else if is(word, "LINESTYLES") {
            linestyles = true;
        } else if is(word, "SYMBOLS") {
            symbols = true;
        } else {
            return Err(usage(EXPORT_USAGE));
        }
        at += 1;
    }
    if only && options.only.is_empty() {
        return Err(usage(EXPORT_USAGE));
    }
    // Which kinds are written. A kind word chooses among them; ONLY chooses
    // among the DEFINITIONS, so with it the codes are written only when CODES
    // says so, and both kinds of definition stay open to the names unless
    // LINESTYLES or SYMBOLS narrows them: CODES ONLY <definition> - the codes
    // and one symbol - must not refuse the very symbol it asks for.
    let part = linestyles || symbols || codes || only;
    if part {
        options.codes = codes;
    }
    if linestyles || symbols {
        options.linestyles = linestyles;
        options.symbols = symbols;
    } else if codes && !only {
        // The codes alone.
        options.linestyles = false;
        options.symbols = false;
    }

    if document.customisation_state().origin == CustomisationOrigin::None {
        return Err(Error::new(
            ErrorCode::InvalidState,
            "no customisation is loaded, so there is nothing to export",
        ));
    }
    let mut session = document.customisation();
    if let Some(given) = &name {
        session.name.clone_from(given);
    }
    if let Err(named) = validate_customisation_name(&session.name) {
        if name.is_none() && session.name.is_empty() {
            return Err(Error::new(
                ErrorCode::InvalidState,
                "the session's customisation has no name to be written under; give one: \
                 CUSTOMISE EXPORT <file> NAME <name>",
            ));
        }
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!("NAME: {}", named.message),
            session.name.as_str(),
        ));
    }
    // After ONLY every word is a definition. One of EXPORT's own words there,
    // naming no definition, is that word put in the wrong place, and "no
    // definition of this name" alone would not say so.
    for word in &options.only {
        if is_export_word(word) && !session.library.contains(word) {
            return Err(make_error(
                ErrorCode::NotFound,
                "after ONLY every word is a definition's name, and the library has no definition \
                 of this name; a word of EXPORT's own goes before ONLY",
                word.as_str(),
            ));
        }
    }

    // The definitions the writer will choose, by the rule it chooses by
    // (entity's CustomisationWriteOptions), counted for the reply.
    let chosen: BTreeSet<&str> = options.only.iter().map(String::as_str).collect();
    let mut definitions = 0usize;
    session.library.for_each(|definition| {
        let kind = if definition.symbol {
            options.symbols
        } else {
            options.linestyles
        };
        if kind && (chosen.is_empty() || chosen.contains(definition.name.as_str())) {
            definitions += 1;
        }
    });
    if part {
        // A part of the session is a fragment for someone else's, or for
        // this one later, cut down by the one rule everything that writes a
        // part cuts by (customisation_part) - the two managers' export
        // buttons too, so that the same symbols are the same file whichever
        // wrote them.
        session = customisation_part(session, &options);
    }
    let text = customisation_to_json(&session, &options)?;
    let replaced = is_there(&file);
    write_whole(&file, text.as_bytes(), None)?;
    Ok(format!(
        "exported file={} name={} definitions={definitions} codes={} rules={} replaced={}",
        path_value(&file),
        record_value(&session.name),
        if options.codes {
            session.map.keys().len()
        } else {
            0
        },
        if options.codes { session.map.size() } else { 0 },
        yes_no(replaced)
    ))
}

// ---- the host's verbs ---------------------------------------------------------------------

// The refusal of a verb that needs what the session was not given, by name.
fn needs_host(verb: &str, what: &str) -> Error {
    Error::new(ErrorCode::InvalidState, format!("CUSTOMISE {verb} {what}"))
}

// Puts `built_in` in the place of the session: the work CUSTOMISE RESET does
// for the built-in of the run and CUSTOMISE BUILTIN <name> for any other the
// host offers, in ONE place so that the two cannot come to differ in what they
// leave. `runs`: `built_in` is the run's own. `named`: it was chosen by name
// (BUILTIN), whose reply says `used` and whether it is the default, where
// RESET's says `reset` and has always said nothing of it.
//
// `kept`, the reply's last field and the session's flag, is what the next
// start would give: the built-in OF THE RUN, and only when no kept file would
// be read in its place. With one there, the session now differs from it until
// a KEEP. Any other built-in is not kept, whatever the file says: the next
// start gives the run's own, and a KEEP is what makes the choice last (it
// writes the session, which says what it is based on).
fn install_chosen(
    document: &mut Document,
    host: &CustomisationHost,
    built_in: &BuiltInCustomisation,
    runs: bool,
    named: bool,
) -> Result<String> {
    let kept = runs && (host.kept_file.as_os_str().is_empty() || !is_there(&host.kept_file));
    install_built_in_customisation(document, built_in, kept)?;
    if runs && let Some(customisation) = &built_in.customisation {
        // What the earlier names of a project are answered with is the run's
        // built-in's name: a session given another built-in does not change it.
        document.set_built_in_customisation_name(customisation.name.clone());
    }
    let mut reply = format!(
        "{} name={}",
        if named { "used" } else { "reset" },
        record_value(&document.customisation_state().name)
    );
    if named {
        reply.push_str(&format!(" default={}", yes_no(runs)));
    }
    reply.push_str(&counts_of(document));
    reply.push_str(&format!(" kept={}", yes_no(kept)));
    Ok(reply)
}

fn reset_to_built_in(
    document: &mut Document,
    context: &CustomisationVerbContext,
) -> Result<String> {
    let Some(host) = &context.host else {
        return Err(needs_host(
            "RESET",
            "needs the program's built-in customisation, and this session was given none",
        ));
    };
    let built_in = &host.built_in;
    if built_in.customisation.is_none() {
        return Err(needs_host(
            "RESET",
            &if built_in.problem.is_empty() {
                String::from("needs a built-in customisation, and this program has none")
            } else {
                format!(
                    "cannot use the built-in customisation: {}",
                    built_in.problem
                )
            },
        ));
    }
    install_chosen(document, host, built_in, true, false)
}

// The words after BUILTIN as one name: an unquoted "Katana Standard" is two
// words by the time a verb sees it, and a name is matched whole.
fn joined_name(words: &[String]) -> String {
    words.join(" ")
}

// CUSTOMISE BUILTIN: the built-ins the host offers, and with a name the choice
// of one. The list is the one answer the JSON report and Settings read too
// (built_in_choices).
fn built_in_verb(
    document: &mut Document,
    context: &CustomisationVerbContext,
    words: &[String],
) -> Result<String> {
    let Some(host) = &context.host else {
        return Err(needs_host(
            "BUILTIN",
            "needs the program's built-in customisations, and this session was given none",
        ));
    };
    let entries = built_ins_of(host);
    if !words.is_empty() {
        let name = joined_name(words);
        let Some(entry) = find_built_in(entries, &name) else {
            let mut have = String::new();
            for each in entries {
                have.push_str(if have.is_empty() { "\"" } else { ", \"" });
                have.push_str(&each.name);
                have.push('"');
            }
            return Err(make_error(
                ErrorCode::NotFound,
                format!(
                    "CUSTOMISE BUILTIN: this program has no built-in customisation of that name, \
                     so nothing was changed; {}",
                    if have.is_empty() {
                        String::from("it has none")
                    } else {
                        format!("it has {have} (CUSTOMISE BUILTIN lists them)")
                    }
                ),
                name,
            ));
        };
        let Some(chosen) = &entry.built_in.customisation else {
            return Err(needs_host(
                "BUILTIN",
                &format!(
                    "cannot use the built-in customisation \"{}\": {}",
                    entry.name, entry.built_in.problem
                ),
            ));
        };
        // The run's own is the one RESET gives; judged as the host judges it.
        let own = &host.built_in;
        let runs = own.customisation.as_ref().is_some_and(|customisation| {
            chosen.name == customisation.name && entry.built_in.digest == own.digest
        });
        return install_chosen(document, host, &entry.built_in, runs, true);
    }

    let choices = built_in_choices(document, entries);
    let mut default_name = String::new();
    let mut active_name = String::new();
    for choice in &choices {
        if choice.is_default {
            default_name.clone_from(&choice.name);
        }
        if choice.is_active {
            active_name.clone_from(&choice.name);
        }
    }
    let mut reply = format!(
        "builtins count={} default={} active={}",
        choices.len(),
        record_value(&default_name),
        record_value(&active_name)
    );
    // A kept or edited copy is not any built-in, but says which it came from.
    let state = document.customisation_state();
    if active_name.is_empty()
        && let Some(based_on) = &state.based_on
    {
        reply.push_str(&format!(" based_on={}", record_value(&based_on.name)));
    }
    for choice in &choices {
        reply.push_str(&format!(
            "\nbuiltin name={} default={} active={} definitions={} codes={} rules={} source={} \
             description={}",
            record_value(&choice.name),
            yes_no(choice.is_default),
            yes_no(choice.is_active),
            choice.definitions,
            choice.codes,
            choice.rules,
            record_value(&choice.source),
            record_value(&choice.description)
        ));
        if !choice.problem.is_empty() {
            reply.push_str(&format!(" problem={}", record_value(&choice.problem)));
        }
    }
    Ok(reply)
}

// Whether a kept file exists, and where, in a sentence: the record's `kept=yes`
// says only that the session is what the next start gives, which with no kept
// file is trivially so (the built-in is what it gives), and reads as a promise
// of a file that is not there.
fn kept_file_sentence(host: &CustomisationHost) -> String {
    if host.kept_file.as_os_str().is_empty() {
        return format!(
            "Kept file: none. This session keeps no file, so each start gives the built-in \
             customisation (kept=yes then means only that nothing is waiting to be kept); the \
             environment variable {KEPT_CUSTOMISATION_VARIABLE} names a file to keep one in."
        );
    }
    // As a sentence says a path: plainly, with '/', not quoted as a record's value is.
    let place = format!("Kept file: {}", generic_text(&host.kept_file));
    if !is_there(&host.kept_file) {
        return format!("{place}, not written yet; CUSTOMISE KEEP writes it.");
    }
    // A file the next start cannot use is not "read" by it: say which kind of
    // trouble it is, because only one of them is something KEEP can move.
    let Ok(bytes) = read_file_bytes(&host.kept_file) else {
        return format!("{place}, which cannot be read, so the next start does not use it.");
    };
    if customisation_from_json(&bytes).is_err() {
        return format!(
            "{place}, which is not a customisation that reads, so the next start does not use \
             it; CUSTOMISE KEEP sets it aside as .bad."
        );
    }
    format!("{place}, which the next start reads.")
}

// The host whose kept file is named, or the refusal that names what is
// missing.
fn kept_file_of<'a>(
    verb: &str,
    context: &'a CustomisationVerbContext,
) -> Result<&'a CustomisationHost> {
    match &context.host {
        Some(host) if !host.kept_file.as_os_str().is_empty() => Ok(host),
        _ => Err(needs_host(
            verb,
            &format!(
                "needs a kept customisation file, and this session has none; the environment \
                 variable {KEPT_CUSTOMISATION_VARIABLE} names one"
            ),
        )),
    }
}

// What the session is when it is the built-in and nothing more: what
// installing it gives. Asked of the install itself, in a Document of its own,
// so that "is the built-in" can never drift from what a RESET leaves.
fn built_in_as_installed(built_in: &BuiltInCustomisation) -> Option<Customisation> {
    built_in.customisation.as_ref()?;
    let mut scratch = Document::default();
    install_built_in_customisation(&mut scratch, built_in, true).ok()?;
    Some(scratch.customisation())
}

fn keep(document: &mut Document, context: &mut CustomisationVerbContext) -> Result<String> {
    let host = kept_file_of("KEEP", context)?;
    let file = host.kept_file.clone();
    let built_in = host.built_in.clone();
    if document.customisation_state().origin == CustomisationOrigin::None {
        return Err(Error::new(
            ErrorCode::InvalidState,
            "CUSTOMISE KEEP: no customisation is loaded, so there is nothing to keep",
        ));
    }
    if document.customisation_state().name.is_empty() {
        // Rules or definitions made in an editor with no customisation ever
        // installed. A file needs a name, and the writer's own refusal - a
        // name cannot be empty - would not say how the session gets one.
        return Err(Error::new(
            ErrorCode::InvalidState,
            "CUSTOMISE KEEP: the session's customisation has no name to be kept under; \
             CUSTOMISE EXPORT <file> NAME <name> writes it under one, and CUSTOMISE REPLACE \
             <file> then gives the session that name",
        ));
    }
    let shown = path_text(&file);

    // What is there now is never written over unseen. Another Katana may
    // have kept its own since this session read the file, and one that
    // cannot be read at all may be one that cannot be put in its place either.
    // A file that was read, and is not a customisation - empty, damaged, or
    // written by a newer Katana - is no loss to put aside: the start that
    // found it used the built-in in its place and said so, and nothing in the
    // window could move it, so the session's own is kept and that file is set
    // aside whole as `.bad` for its owner to mend or to read with the Katana
    // that wrote it.
    let there = is_there(&file);
    // The bytes that are there, as they were judged.
    let mut previous: Vec<u8> = Vec::new();
    // There, and not a customisation.
    let mut unreadable = false;
    if there {
        let bytes = read_file_bytes(&file).map_err(|error| {
            make_error(
                ErrorCode::InvalidState,
                format!(
                    "CUSTOMISE KEEP: the kept customisation file cannot be read, so it is not \
                     written over: {}",
                    error.message
                ),
                shown.as_str(),
            )
        })?;
        if context.kept_digest.as_deref() != Some(customisation_digest(&bytes).as_str()) {
            return Err(make_error(
                ErrorCode::InvalidState,
                "CUSTOMISE KEEP: the kept customisation changed on disk since this session read \
                 it, so it is not written over; CUSTOMISE REVERT reads it (CUSTOMISE EXPORT \
                 <file> first keeps what this session has)",
                shown.as_str(),
            ));
        }
        unreadable = customisation_from_json(&bytes).is_err();
        previous = bytes;
    }

    // Where what is there goes: the earlier edition of a customisation beside
    // it as `.bak`, one that is not a customisation aside as `.bad`.
    let backup = beside(&file, ".bak");
    let bad = beside(&file, ".bad");
    let aside = if unreadable { &bad } else { &backup };
    let aside_fields = format!(
        " backup={}{}",
        if there && !unreadable {
            path_value(&backup)
        } else {
            String::from("none")
        },
        if unreadable {
            format!(" set_aside={}", path_value(&bad))
        } else {
            String::new()
        }
    );
    let session = document.customisation();
    if built_in_as_installed(&built_in).is_some_and(|installed| session == installed) {
        // Every start gives the built-in when nothing is kept. A copy of it
        // kept all the same would be read in its place at every later start,
        // and so would hide every later edition of the built-in for ever.
        //
        // Moved over the file of that name, which the rename replaces, so the
        // earlier backup is never absent: a failed rename leaves both files.
        if there && let Err(error) = fs::rename(&file, aside) {
            return Err(make_error(
                ErrorCode::FileExportFailure,
                "CUSTOMISE KEEP: the kept customisation could not be set aside",
                format!("{shown}: {}", os_message(&error)),
            ));
        }
        context.kept_digest = None;
        document.set_customisation_kept(true);
        return Ok(format!(
            "kept file={} written=no reason=the-session-is-the-built-in{aside_fields}",
            path_value(&file)
        ));
    }

    let text = customisation_to_json(&session, &CustomisationWriteOptions::default()).map_err(
        |error| {
            make_error(
                error.code,
                format!(
                    "CUSTOMISE KEEP: the session cannot be written: {}",
                    error.message
                ),
                error.context,
            )
        },
    )?;
    let replaced = Replaced {
        beside: aside.clone(),
        bytes: &previous,
    };
    write_whole(
        &file,
        text.as_bytes(),
        if there { Some(&replaced) } else { None },
    )?;
    context.kept_digest = Some(customisation_digest(text.as_bytes()));
    document.set_customisation_kept(true);
    Ok(format!(
        "kept file={} written=yes name={}{}{aside_fields}",
        path_value(&file),
        record_value(&session.name),
        counts_of(document)
    ))
}

fn revert(document: &mut Document, context: &mut CustomisationVerbContext) -> Result<String> {
    let file = kept_file_of("REVERT", context)?.kept_file.clone();
    if !is_there(&file) {
        return Err(make_error(
            ErrorCode::NotFound,
            "CUSTOMISE REVERT: no customisation is kept, so there is none to go back to; \
             CUSTOMISE RESET gives the built-in one",
            path_text(&file),
        ));
    }
    let read = read_customisation_file(&file)?;
    document.install_customisation(read.customisation, CustomisationOrigin::Kept, true)?;
    context.kept_digest = Some(read.digest);
    Ok(format!(
        "reverted file={} name={}{}",
        path_value(&file),
        record_value(&document.customisation_state().name),
        counts_of(document)
    ))
}

// ---- removing -----------------------------------------------------------------------------

// What each name's record counts: the rules, the styles (each once - the
// shared answer lists a style under its linetype AND under its symbol when it
// names the definition both ways) and the layers that name it.
struct Counted {
    rules: usize,
    styles: usize,
    layers: usize,
}

// What names a definition is asked of definition_users - the function the
// window's definition editor lists a definition's users with, so that what
// REMOVE refuses for is what a person is shown beside Delete: the survey code
// rules that give it as their linestyle or their symbol, the drawing's styles
// that give it as their linetype or their symbol, and the drawing's layers
// that give it as their linetype (an entity with no style of its own is drawn
// with its layer's). A name is a use as the resolver would draw it: the name
// itself, or the earlier name of a definition since renamed.
fn remove_definitions(document: &mut Document, names: &[String], force: bool) -> Result<String> {
    let names = distinct(names);
    if names.is_empty() {
        return Err(usage(REMOVE_USAGE));
    }
    let mut library = document.style_library().clone();
    let mut unknown = String::new();
    for name in &names {
        if !library.contains(name) {
            unknown.push_str(if unknown.is_empty() { "\"" } else { ", \"" });
            unknown.push_str(name);
            unknown.push('"');
        }
    }
    if !unknown.is_empty() {
        return Err(make_error(
            ErrorCode::NotFound,
            "CUSTOMISE REMOVE: the library has no definition of that name, so nothing was \
             removed",
            unknown,
        ));
    }

    let mut uses: Vec<Counted> = Vec::new();
    let mut used = 0usize;
    let mut listing = String::new();
    for name in &names {
        let users = definition_users(document, name);
        uses.push(Counted {
            rules: users.rules.len(),
            styles: users.styles().len(),
            layers: users.layers.len(),
        });
        used += usize::from(!users.empty());
        // In the words the window's editor looks for under its Delete
        // (DefinitionUsers::cited), so it does not list the users again.
        for line in users.cited(name) {
            listing.push_str("\n  ");
            listing.push_str(&line);
        }
    }
    if used != 0 && !force {
        return Err(Error::new(
            ErrorCode::InvalidState,
            format!(
                "CUSTOMISE REMOVE: {used} of the {}{} in use, so nothing was removed; FORCE \
                 removes what is used too, and what names it then draws plain{listing}",
                names.len(),
                if names.len() == 1 {
                    " definition named is"
                } else {
                    " definitions named are"
                }
            ),
        ));
    }
    for name in &names {
        library.remove(name)?;
    }
    document.set_style_library(library);
    let mut reply = String::new();
    for (name, counted) in names.iter().zip(&uses) {
        reply.push_str(&format!(
            "removed definition={} rules={} styles={} layers={}\n",
            record_value(name),
            counted.rules,
            counted.styles,
            counted.layers
        ));
    }
    Ok(trimmed_reply(reply))
}

fn remove_codes(document: &mut Document, keys: &[String]) -> Result<String> {
    let keys = distinct(keys);
    if keys.is_empty() {
        return Err(usage(REMOVE_USAGE));
    }
    let mut counts = vec![0usize; keys.len()];
    let mut kept = SurveyMap::new();
    for rule in document.survey_map().rules() {
        if let Some(found) = keys.iter().position(|key| *key == rule.key) {
            counts[found] += 1;
            continue;
        }
        // Cannot fail: the rule is in a map, which took it.
        kept.add(rule.clone())?;
    }
    let mut unknown = String::new();
    for (key, count) in keys.iter().zip(&counts) {
        if *count == 0 {
            unknown.push_str(if unknown.is_empty() { "\"" } else { ", \"" });
            unknown.push_str(key);
            unknown.push('"');
        }
    }
    if !unknown.is_empty() {
        // A key is matched as it is written, byte for byte, as a code is.
        return Err(make_error(
            ErrorCode::NotFound,
            "CUSTOMISE REMOVE CODE: no rule has that key, so nothing was removed",
            unknown,
        ));
    }
    document.set_survey_map(kept);
    let mut reply = String::new();
    for (key, count) in keys.iter().zip(&counts) {
        reply.push_str(&format!(
            "removed code={} rules={count}\n",
            record_value(key)
        ));
    }
    Ok(trimmed_reply(reply))
}

fn remove_from(document: &mut Document, args: &[String]) -> Result<String> {
    // CODE is the keyword only as the first word, and FORCE only as the last.
    if let Some(first) = args.first()
        && is(first, "CODE")
    {
        return remove_codes(document, &args[1..]);
    }
    let force = args.last().is_some_and(|last| is(last, "FORCE"));
    let end = args.len() - usize::from(force);
    remove_definitions(document, &args[..end], force)
}

// ---- settings -----------------------------------------------------------------------------

fn set_values(document: &mut Document, args: &[String]) -> Result<String> {
    if args.is_empty() {
        return Err(usage(SET_USAGE));
    }
    // Everything is read before anything is set, so a line with one bad item
    // sets none of them.
    let mut automation = document.customisation_state().automation;
    let mut linework = document.customisation_state().linework.clone();
    let mut reply = String::from("set");
    let mut given: BTreeSet<String> = BTreeSet::new();
    for word in args {
        let equals = match word.find('=') {
            Some(at) if at != 0 => at,
            _ => {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    format!("a SET item is key=value; usage: {SET_USAGE}"),
                    word.as_str(),
                ));
            }
        };
        let key = word[..equals].to_ascii_lowercase();
        let value = &word[equals + 1..];
        // A key given twice is two answers to one question. The later one
        // won, and the reply said both back as though both had been set.
        if !given.insert(key.clone()) {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                format!("a SET key is given once in a line, and {key} is given twice"),
                word.as_str(),
            ));
        }
        if key == "auto.codes" || key == "auto.linework" {
            let on = is(value, "on");
            if !on && !is(value, "off") {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    format!("{key} is on or off"),
                    word.as_str(),
                ));
            }
            if key == "auto.codes" {
                automation.codes_on_survey_import = on;
            } else {
                automation.linework_on_survey_import = on;
            }
            reply.push_str(&format!(" {key}={}", if on { "on" } else { "off" }));
            continue;
        }
        let mut known = false;
        for member in linework_code_members() {
            if key == format!("linework.{}", member.name.to_ascii_lowercase()) {
                // An empty spelling switches the control off.
                *member.spelling_mut(&mut linework) = String::from(value);
                reply.push_str(&format!(" {key}={}", record_value(value)));
                known = true;
                break;
            }
        }
        if !known {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                format!("not a SET key; usage: {SET_USAGE}"),
                word.as_str(),
            ));
        }
    }
    // The codes first: they are the one part that can be refused, and
    // refused they leave the switches unset too.
    if let Err(error) = document.set_linework_codes(linework) {
        return Err(make_error(
            error.code,
            format!(
                "CUSTOMISE SET: the linework codes would be refused, so nothing was set: {}",
                error.message
            ),
            error.context,
        ));
    }
    document.set_automation(automation);
    Ok(reply)
}

/// The context of a session that begins now: `host`, and its kept file as it
/// is at this moment. So hand it over where the session starts - BEFORE
/// start_customisation reads the file, not after: read the other way round, a
/// file another Katana wrote between the two would be taken for the one this
/// session saw.
pub fn customisation_verb_context(host: CustomisationHost) -> CustomisationVerbContext {
    CustomisationVerbContext {
        kept_digest: digest_of_file(&host.kept_file),
        host: Some(host),
    }
}

/// True for CUSTOMISE, in any case. (CUSTOMIZE is the interpreter's alias.)
pub fn is_customisation_verb(verb: &str) -> bool {
    is(verb, "CUSTOMISE")
}

/// Runs one CUSTOMISE line: `args` are the words AFTER the verb, the quotes
/// removed (CommandInterpreter::tokenize). A refused line changes nothing:
/// neither the session nor a file. `context` is updated by KEEP and REVERT.
pub fn run_customisation_verb(
    document: &mut Document,
    args: &[String],
    context: &mut CustomisationVerbContext,
) -> Result<String> {
    let Some(word) = args.first() else {
        let mut reply = trimmed_reply(format_customisation_reply(&customisation_summary(document)));
        if let Some(host) = &context.host {
            reply.push('\n');
            reply.push_str(&kept_file_sentence(host));
        }
        return Ok(reply);
    };
    let rest = &args[1..];
    // The words that take nothing after them say so, rather than read a
    // second word as a file or ignore it.
    for alone in ["JSON", "RESET", "KEEP", "REVERT"] {
        if is(word, alone) && !rest.is_empty() {
            return Err(usage(&format!("CUSTOMISE {alone}")));
        }
    }
    if is(word, "JSON") {
        // The built-ins it lists are the host's, as CUSTOMISE BUILTIN lists them.
        return Ok(match &context.host {
            Some(host) => customisation_json_offering(document, built_ins_of(host)),
            None => customisation_json(document),
        });
    }
    if is(word, "REPLACE") {
        return load(document, context, rest, LoadMode::Replace, false);
    }
    if is(word, "DEFINITIONS") {
        return load(document, context, rest, LoadMode::Merge, true);
    }
    if is(word, "EXPORT") {
        return export_to(document, rest);
    }
    if is(word, "RESET") {
        return reset_to_built_in(document, context);
    }
    if is(word, "BUILTIN") {
        return built_in_verb(document, context, rest);
    }
    if is(word, "KEEP") {
        return keep(document, context);
    }
    if is(word, "REVERT") {
        return revert(document, context);
    }
    if is(word, "REMOVE") {
        return remove_from(document, rest);
    }
    if is(word, "SET") {
        return set_values(document, rest);
    }
    load(document, context, args, LoadMode::Merge, false)
}

/// Every word of the family and every reply, for HELP CUSTOMISE.
pub fn customisation_verb_help() -> String {
    let text = r#"CUSTOMISE: the session's customisation - its linestyle and symbol definitions, its survey
code rules, colours, linework codes and automation - read and written as one Katana
customisation file (docs/customisation.md). A keyword is the whole FIRST word, in any case; a
file called as one is given with its directory (./json). CUSTOMIZE is the same verb. A reply
is records, one a line, a file's path written with /.

Report    CUSTOMISE   the counts, then: customisation name= origin=none|builtIn|kept|loaded|
          edited kept=yes|no definitions= codes= rules= colours=  |  source name=
          definitions=yes|no rules=yes|no, one a source  |  automation auto.codes=
          auto.linework=  |  linework linework.start= ... (both as SET takes them)  |  missing
          name=, one a name the open project recorded that is not loaded; then what this
          drawing uses of it; then, in a session given a host, one sentence: Kept file: none |
          <path>, not written yet | <path>, which the next start reads | <path>, which does not
          read (kept=yes says only that nothing waits to be kept, which with no kept file
          is always so)
          CUSTOMISE JSON   the same as one JSON object, with the notices, the colours, what
          is wrong with the rules, and the coverage
Load      CUSTOMISE <file> [<file>...]   merge the files into what is loaded: a definition
          takes the place of the one of its name, and the rules a file gives a key in a
          section take the place of that key's rules there
          CUSTOMISE REPLACE <file> [<file>...]   in the place of each KIND the files bring:
          their definitions become the whole library, their rules the whole of the codes; a
          kind they do not bring is kept
          All or nothing: when one file does not read, or one thing in one is refused, none
          is loaded, and every reason is listed. A file named twice is read once (repeated
          file=). A file of another program is not a Katana customisation file.
          loaded file= name= definitions_added= definitions_replaced= codes_added=
          codes_replaced= colours_added= colours_replaced= linework=yes|no automation=yes|no,
          one a file  |  removed definitions= codes= (REPLACE)  |  undefined names= (what the
          rules ask for and nothing defines), each with up to 20 of the names; names the
          built-in itself leaves undefined say in_built_in=yes, counted as in_built_in=<k> on
          the first line when k is not 0  |  the customisation record
          CUSTOMISE DEFINITIONS <file> [<file>...]   merge only the definitions and the colours
          of the files (what the Symbol Library's Import Definitions loads): the rules, the
          linework codes and the automation they hold are left, and the reply says so:
          left rules= linework=yes|no automation=yes|no. Refused when they hold neither
          a definition nor a colour
Export    CUSTOMISE EXPORT <file> [CODES] [LINESTYLES] [SYMBOLS] [NAME <name>]
          [ONLY <definition>...]   write the session; with a kind word, those kinds alone;
          ONLY, those definitions, of either kind unless LINESTYLES or SYMBOLS says which
          (and no codes unless CODES is said); NAME, under that name.
          A part is written without the linework codes and the automation, so that merging it
          resets nobody's, and lists as its sources only those it holds something of: where
          it is loaded a source is taken at its word. The colours its rules and pens name go
          with it, and every notice. The window's two export buttons write a part the same way.
          A file that is there is written over, and the reply says so:
          exported file= name= definitions= codes= rules= replaced=yes|no
          The file is the first word; a file called as one of EXPORT's own words is given
          with its directory (./CODES). After ONLY every word is a definition; a definition
          called NAME is listed by giving NAME <name> before ONLY
Start     CUSTOMISE RESET   the built-in customisation of this run in the place of the session's -
          @DEFAULT@ unless KATANA_BUILTIN_CUSTOMISATION says another:
          reset name= definitions= codes= rules= kept=yes|no
          CUSTOMISE BUILTIN   the built-in customisations this program has, the default marked and
          the one the session is: builtins count= default= active= [based_on=<the built-in a kept
          or edited session was made from, when it is none of them>], then builtin name=
          default=yes|no active=yes|no definitions= codes= rules= source=<the file compiled in>
          description=, one a built-in
          CUSTOMISE BUILTIN <name>   that built-in in the place of the session's, as RESET does
          (the name in any letter case, quoted or not): used name= default=yes|no definitions=
          codes= rules= kept=yes|no. kept=yes only for the run's own built-in with no kept file
          waiting; any other is not what the next start gives until a KEEP
          CUSTOMISE KEEP   write the session to the kept file, which the next start reads in
          the place of the built-in; the file that was there is kept beside it as .bak:
          kept file= written=yes name= definitions= codes= rules= backup=
          When the session IS the built-in of the run nothing is written and the kept file is set
          aside (written=no reason=the-session-is-the-built-in), so that a later built-in is not
          hidden by a copy of this one. Another built-in of the program (CUSTOMISE BUILTIN
          <name>) is written like any session, and the file says which it is based on: the next
          start comes up as it. A kept file that does not read - empty, damaged, or
          written by a newer Katana - is put aside whole as .bad (one of that name is
          replaced) and the session written: backup=none set_aside=<the .bad>. Refused when
          the file changed on disk since this session read it, or cannot be read at all
          CUSTOMISE REVERT   read the kept file again: reverted file= name= definitions= codes=
          rules=
          These three need what the program was started with; a session given no built-in or
          no kept file refuses each by name
Remove    CUSTOMISE REMOVE <definition>... [FORCE]   refused, listing the rules and the
          drawing's styles and layers that name one, unless FORCE: removed definition=
          rules= styles= layers=
          CUSTOMISE REMOVE CODE <key>...   every rule of each key: removed code= rules=
          (To CHANGE a rule or a definition, merge a file that holds it: CUSTOMISE <file>)
Settings  CUSTOMISE SET <key>=<value>...   auto.codes=on|off and auto.linework=on|off (code a
          survey import's points, then string them); linework.start, .end, .close, .arcstart,
          .arcend, .join and .rectangle=<word>, a control's spelling in a field code (empty:
          that control is off). A key is given once in a line, and one refused item sets
          none: set <key>=<value> ...
Editing a customisation changes the session only: KEEP makes it what the next start gives."#;
    // The default is whatever this build compiled in as its default: its name
    // is read, not written here, since a build may be given another file.
    let the_default = match &compiled_in_customisation().customisation {
        Some(customisation) => format!("the default, {},", customisation.name),
        None => String::from("the default (this build has none),"),
    };
    text.replacen("@DEFAULT@", &the_default, 1)
}
