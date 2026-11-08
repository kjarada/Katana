//! Which customisations are loaded, and what a project records of them
//! (include/katana/cad/customisation_record.hpp;
//! src/katana_cad/customisation/customisation_record.cpp).
//!
//! The customisation is SESSION data (decision D1): the library and the survey
//! map live on the Document, are not undoable and are not saved in the
//! project. What a project does keep is a RECORD of what it was drawn with, by
//! name (storage's ProjectMetadata::customisation), so that opening it where
//! that is not loaded can say which names are missing rather than drawing
//! plain lines without a word.
//!
//! The Document keeps the list of what was loaded and applies these functions
//! itself: Document::open works out what is missing and a save writes the
//! record into what it saves (document.rs, "the customisation's session
//! state"). They are functions over plain lists so that the rules can be
//! tested without a project on disk. A source arrives here as its name and
//! what it brought, whoever read it.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

use katana_core::text::is_ascii_space;
use katana_entity::StyleLibrary;

/// One loaded customisation, by name (never a path: a project travels between
/// machines), with what it brought and its author's notice: {name,
/// definitions, rules, notice}. It is the very entry a Katana customisation
/// file keeps in its "sources" (entity's customisation.rs), so the session's
/// load record is written and read back without a second type.
///
/// Two flags, because one Katana customisation brings both definitions and
/// rules, and a record that could say only one of them would lose the source
/// at the first Replace of the other kind. A source's NAME is its identity:
/// two entries of one name are one source.
pub type CustomisationSource = katana_entity::CustomisationSourceNote;

/// Adds a load to `loaded`, which is in load order. A source loaded again
/// moves to the end, where the load puts it, still bringing what it brought
/// before as well. `replaced_definitions` / `replaced_rules`: the load
/// REPLACED that kind (LoadMode::Replace, and it brought some), so what the
/// earlier sources brought of it is gone - and a source that then brings
/// neither kind goes with it.
pub fn record_customisation_load(
    loaded: &mut Vec<CustomisationSource>,
    load: &[CustomisationSource],
    replaced_definitions: bool,
    replaced_rules: bool,
) {
    // Replaced: what the earlier sources brought of that kind is gone. One
    // that brought the other kind as well still brings that; one that now
    // brings neither goes. A source that never brought either - a table of
    // colours - is no kind a Replace takes the place of, and stays.
    let mut standing = Vec::with_capacity(loaded.len() + load.len());
    for mut source in std::mem::take(loaded) {
        let brought = source.definitions || source.rules;
        source.definitions = source.definitions && !replaced_definitions;
        source.rules = source.rules && !replaced_rules;
        if !brought || source.definitions || source.rules {
            standing.push(source);
        }
    }
    *loaded = standing;
    for source in load {
        let mut entry = source.clone();
        // Loaded again - or named twice in this load, which read it twice and
        // is still one source: it is now as recent as this load, and what it
        // brought before and still brings is its own as well.
        if let Some(earlier) = loaded.iter().position(|one| one.name == source.name) {
            let earlier = loaded.remove(earlier);
            entry.definitions = entry.definitions || earlier.definitions;
            entry.rules = entry.rules || earlier.rules;
            if entry.notice.is_empty() {
                entry.notice = earlier.notice;
            }
        }
        loaded.push(entry);
    }
}

/// The names a project records: the loaded sources in load order, then, in
/// name order, any definition's LineStyle::source no loaded source accounts
/// for. A source that brought definitions ALONE is recorded only while some
/// definition of `library` still comes from it (a later load can have
/// replaced every one); rules carry no source name, so a source that brought
/// any is taken at its word.
pub fn customisation_record(library: &StyleLibrary, loaded: &[CustomisationSource]) -> Vec<String> {
    let mut sources: BTreeSet<String> = BTreeSet::new();
    library.for_each(|definition| {
        if !definition.source.is_empty() {
            sources.insert(definition.source.clone());
        }
    });
    let mut names: Vec<String> = Vec::new();
    for source in loaded {
        // Rules carry no source name, so a source that brought any is taken at
        // its word, and so is one that brought neither kind (its colours are
        // still what names are drawn in); one that brought definitions alone
        // is recorded only while it still defines something.
        let definitions_alone = source.definitions && !source.rules;
        if (!definitions_alone || sources.contains(&source.name)) && !names.contains(&source.name) {
            names.push(source.name.clone());
        }
    }
    // Definitions from a file no load here accounts for - the library was
    // given some other way - are still what the drawing is drawn with.
    for source in sources {
        if !names.contains(&source) {
            names.push(source);
        }
    }
    names
}

/// A source that is now loaded under a name other than the one a project may
/// have recorded for it. `former_name` is source_name_hash of that earlier
/// name, not the name itself: see builtin_renames for why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenamedSource {
    pub former_name: u64,
    /// The name that has its place now. Empty answers nothing: the table of a
    /// program with no built-in customisation has nothing to answer with.
    pub now: String,
    /// Whether the earlier name is particular enough to be known by on its
    /// own. A plain one - a name anyone's own file might have - is taken for
    /// the renamed source only in the record of the set the rename was made
    /// to: one that also holds ANOTHER earlier name of the same `set`,
    /// distinctive or plain. Alone, or beside names of other sets, it is some
    /// other file, and passing it as loaded would hide that it is missing.
    pub distinctive: bool,
    /// Which generation of earlier names this one belongs to. One
    /// customisation has been recorded under several sets of names over time,
    /// and a record made at one time holds the names of one set: names of two
    /// sets beside each other are evidence of neither.
    pub set: i32,
}

impl Default for RenamedSource {
    fn default() -> Self {
        Self {
            former_name: 0,
            now: String::new(),
            distinctive: true,
            set: 0,
        }
    }
}

/// FNV-1a (64-bit) of the name's bytes, exactly as spelt: the key
/// RenamedSource knows an earlier name by.
pub fn source_name_hash(name: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325; // the FNV-1a 64-bit offset basis
    for &byte in name.as_bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3); // the FNV 64-bit prime
    }
    hash
}

/// WHOSE files the earlier names were: the NSW customisation's. A program has
/// several built-ins now and the default is another, so the earlier names are
/// answered by the one called this and by no other: under Katana Standard,
/// "Katana Standard" defines none of what they drew, and a project drawn with
/// them is told that its customisation is not loaded until NSW is (CUSTOMISE
/// BUILTIN NSW). builtin_renames_default takes this, which is what the
/// Document passes, whatever its host's built-in is - the table does not
/// follow the run's built-in.
pub const EARLIER_FILE_NAMES_BELONG_TO: &str = "NSW";

/// The names a project may have recorded for the built-in customisation before
/// it was ONE customisation with a name of its own, each answered by that
/// name, `built_in`: eight entries in two sets.
///
/// * Set 0: the four files it was first shipped as, in load order. Their names
///   carried the name of the customisation's publisher, which has no place in
///   this source, so they are known by hash. The second survey code file's
///   name was a plain one, without the publisher's, and is marked so
///   (RenamedSource::distinctive).
/// * Set 1: the general names those four files were given next, in load order
///   ("linestyles.4d", "survey_codes.mapfile", "survey_codes_names.mapfile" and
///   "symbols.4d"). All four are plain, names anyone's own files might have,
///   so each is answered only beside another of the four: the record of the
///   built-in holds all four, and a person's own "symbols.4d" recorded alone
///   is still that person's file, missing when it is.
///
/// The functions below take the table they are given, and have no form that
/// guesses one.
pub fn builtin_renames(built_in: &str) -> Vec<RenamedSource> {
    // The four files' first names, hashed by source_name_hash, in load order.
    // Written out by a script over the two sets of names when the files were
    // given general ones: nothing here can spell them.
    const FIRST_NAMES: [(u64, bool); 4] = [
        (0x851e5f2b3efc2d70, true), // the linestyle library
        (0x4203f483f25be01e, true), // the first survey code file
        // A plain name, one a person's own file may well have.
        (0xc03aa2ccdeee0553, false), // the second survey code file
        (0x01bf8dab227e48e7, true),  // the symbol library
    ];
    // The names the four files had next, until the built-in became one
    // customisation: general names, so every one of them is plain.
    const GENERAL_NAMES: [&str; 4] = [
        "linestyles.4d",
        "survey_codes.mapfile",
        "survey_codes_names.mapfile",
        "symbols.4d",
    ];

    let mut renames = Vec::with_capacity(FIRST_NAMES.len() + GENERAL_NAMES.len());
    for (hash, distinctive) in FIRST_NAMES {
        renames.push(RenamedSource {
            former_name: hash,
            now: String::from(built_in),
            distinctive,
            set: 0,
        });
    }
    for name in GENERAL_NAMES {
        renames.push(RenamedSource {
            former_name: source_name_hash(name),
            now: String::from(built_in),
            distinctive: false,
            set: 1,
        });
    }
    renames
}

/// builtin_renames of EARLIER_FILE_NAMES_BELONG_TO: the C++ `builtinRenames()`.
pub fn builtin_renames_default() -> Vec<RenamedSource> {
    builtin_renames(EARLIER_FILE_NAMES_BELONG_TO)
}

/// The rename of `renamed` whose earlier name `name` is, if any.
fn rename_of<'a>(name: &str, renamed: &'a [RenamedSource]) -> Option<&'a RenamedSource> {
    let hash = source_name_hash(name);
    renamed.iter().find(|rename| rename.former_name == hash)
}

/// The rename that answers for `name` in a project's `record`: its own, when
/// the earlier name is distinctive or the record also holds ANOTHER earlier
/// name of the same set - the record of the renamed set, not of someone's own
/// file that shares a plain name. None, when the rename has no name to answer
/// with. customisation_not_loaded, note_customisation_loaded and
/// customisation_record_to_save all ask this, so they cannot come to disagree
/// about which recorded names a rename answers.
fn rename_answering<'a>(
    name: &str,
    record: &[String],
    renamed: &'a [RenamedSource],
) -> Option<&'a RenamedSource> {
    let rename = rename_of(name, renamed)?;
    if rename.now.is_empty() {
        return None;
    }
    if rename.distinctive {
        return Some(rename);
    }
    let renamed_set = record.iter().any(|other| {
        // The name itself is no company: recorded twice, it is still alone.
        if other == name {
            return false;
        }
        rename_of(other, renamed).is_some_and(|beside| beside.set == rename.set)
    });
    renamed_set.then_some(rename)
}

/// Of the names a project `recorded`, those that are not in
/// customisation_record(library, loaded), in the project's order: what to warn
/// about when the project is opened. Names compare exactly: a file loaded
/// under another spelling is reported as missing, which a warning can afford,
/// where the reverse - a missing file passed as loaded - it could not.
///
/// The one exception is a file RENAMED since the project was saved: a recorded
/// name `renamed` knows is answered by the file that now has its place, and is
/// missing only when that file is. A project saved before the built-in
/// customisation was given general names records the old names, and what
/// those files brought is loaded under the new ones; telling a person four
/// files are missing, and keeping them in the record for ever after, when
/// nothing is, would be a false alarm nobody could clear. A plain earlier name
/// (RenamedSource::distinctive false) is answered this way only beside another
/// earlier name of its set, so a person's own file of that name is still
/// missing when it is. `renamed` is the caller's to give, here and below, and
/// has no default: a table answers with ONE built-in's name, and only the
/// session knows its own.
pub fn customisation_not_loaded(
    recorded: &[String],
    library: &StyleLibrary,
    loaded: &[CustomisationSource],
    renamed: &[RenamedSource],
) -> Vec<String> {
    let now = customisation_record(library, loaded);
    let is_now = |name: &str| now.iter().any(|loaded_name| loaded_name == name);
    let mut missing: Vec<String> = Vec::new();
    for name in recorded {
        if is_now(name.as_str()) || missing.contains(name) {
            continue;
        }
        // Recorded under a name its file no longer has: answered by the file
        // that has its place, when that file is loaded now.
        if rename_answering(name, recorded, renamed)
            .is_some_and(|rename| is_now(rename.now.as_str()))
        {
            continue;
        }
        missing.push(name.clone());
    }
    missing
}

/// A load's files, each once, in the order first named: read twice, a file's
/// every rule would sit in the map twice, since both copies are the load's and
/// the merge keeps them all. `repeats` holds each later naming, as named, for
/// the front end to say it was read once. Both front ends (the window's
/// CUSTOMISE and File > Settings > Import, the command line's CUSTOMISE) go
/// through this, so they cannot differ on the same line.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DistinctFiles {
    /// Absolute and lexically normal: what is read.
    pub files: Vec<PathBuf>,
    pub repeats: Vec<PathBuf>,
}

/// std::filesystem's lexically_normal on the components Rust's Path already
/// splits: "." gone, a name followed by ".." gone with it, a ".." straight
/// after the root gone, and "." for nothing at all.
fn lexically_normal(path: &Path) -> PathBuf {
    let mut kept: Vec<Component<'_>> = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => match kept.last() {
                Some(Component::Normal(_)) => {
                    kept.pop();
                }
                Some(Component::RootDir) => {}
                _ => kept.push(component),
            },
            other => kept.push(other),
        }
    }
    if kept.is_empty() {
        return PathBuf::from(".");
    }
    kept.iter().collect()
}

/// std::filesystem::equivalent, false where it would report an error (a file
/// that does not exist): the same device and file, by the file system's own
/// identity of a file.
#[cfg(unix)]
fn equivalent(first: &Path, second: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (std::fs::metadata(first), std::fs::metadata(second)) {
        (Ok(a), Ok(b)) => a.dev() == b.dev() && a.ino() == b.ino(),
        _ => false,
    }
}

/// The C++ compares the volume and the file index (GetFileInformationByHandle),
/// which stable Rust does not expose; the final, canonical paths of two files
/// are one where both are the same file reached by another letter case or a
/// link. Two hard links to one file are not told apart, so such a pair is
/// read twice, which is the side this function errs on by design.
#[cfg(windows)]
fn equivalent(first: &Path, second: &Path) -> bool {
    match (std::fs::canonicalize(first), std::fs::canonicalize(second)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Two namings are one file when their absolute, lexically normal paths are
/// equal, or when both exist and are equivalent (another letter case on
/// Windows, a link). A pair it cannot tell apart is read twice, which doubles
/// rules; it never drops a file.
pub fn distinct_customisation_files(named: &[PathBuf]) -> DistinctFiles {
    let mut distinct = DistinctFiles::default();
    for path in named {
        let file = match std::path::absolute(path) {
            Ok(absolute) => lexically_normal(&absolute),
            // Unresolvable here; the read that follows says why.
            Err(_) => lexically_normal(path),
        };
        let repeat = distinct
            .files
            .iter()
            .any(|seen| *seen == file || equivalent(seen, &file));
        if repeat {
            distinct.repeats.push(path.clone());
        } else {
            distinct.files.push(file);
        }
    }
    distinct
}

/// After a load: the names opening the project found not loaded
/// (`missing_at_open`, from customisation_not_loaded) that this load brought
/// are no longer missing, and the session is now the judge of them. A name the
/// load brought under the name `renamed` gives it now counts as brought, as it
/// does for customisation_not_loaded, so loading the files a warning asks for
/// clears it; a plain earlier name counts only while another earlier name of
/// its set is missing beside it, `missing_at_open` being all of the record
/// this sees.
pub fn note_customisation_loaded(
    missing_at_open: &mut Vec<String>,
    load: &[CustomisationSource],
    renamed: &[RenamedSource],
) {
    // Judged against the list as the load found it, not as it shrinks.
    let before = missing_at_open.clone();
    missing_at_open.retain(|name| {
        let rename = rename_answering(name, &before, renamed);
        !load
            .iter()
            .any(|file| file.name == *name || rename.is_some_and(|rename| file.name == rename.now))
    });
}

/// The record a save writes: customisation_record(library, loaded) - what this
/// session draws the drawing with - then, in the project's order, the names
/// the project already `recorded` that are still in `missing_at_open` and that
/// customisation_not_loaded still finds missing. A file the open found missing
/// and no load has brought since is one this session cannot judge: saving
/// without it must not forget it, or every later open, anywhere, would draw
/// plain lines without a word. One this session loaded and then replaced, it
/// has judged, and it goes; so does an earlier name whose file is loaded now
/// under the name `renamed` gives it, judged against the whole record. A
/// person's own file that shares a plain earlier name is kept while it is
/// missing. `recorded` is the document's current record, so a list left over
/// from an earlier drawing keeps nothing a new one never recorded.
pub fn customisation_record_to_save(
    recorded: &[String],
    missing_at_open: &[String],
    library: &StyleLibrary,
    loaded: &[CustomisationSource],
    renamed: &[RenamedSource],
) -> Vec<String> {
    let mut names = customisation_record(library, loaded);
    // What the whole record still lacks: a missing earlier name that
    // note_customisation_loaded could not clear, seeing only the missing
    // names, is judged here against the record, as the open judged it.
    let still_missing = customisation_not_loaded(recorded, library, loaded, renamed);
    for name in recorded {
        if missing_at_open.contains(name) && still_missing.contains(name) && !names.contains(name) {
            names.push(name.clone());
        }
    }
    names
}

/// A definition of the built-in customisation that has another name than the
/// one a drawing saved earlier may give it (as a style's linetype, or a placed
/// symbol's name). `former_name` is source_name_hash of the earlier name, for
/// the reason builtin_renames gives.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RenamedDefinition {
    pub former_name: u64,
    pub now: &'static str,
}

const fn renamed_definition(former_name: u64, now: &'static str) -> RenamedDefinition {
    RenamedDefinition { former_name, now }
}

/// Each definition's earlier name, hashed by source_name_hash, and the name it
/// has now: the earlier name with the publisher's leading word taken off.
/// Written out by a script over the definitions before and after the rename;
/// the tests check the names given now against the built-in customisation
/// itself.
const BUILTIN_DEFINITION_RENAMES: [RenamedDefinition; 29] = [
    renamed_definition(0x063e2ef59dcfd3b6, "Accepted For Construction"),
    renamed_definition(0xcc0997296f480f64, "Horizontal Scale 1 to 100"),
    renamed_definition(0xfc5b2f6817720bbc, "Horizontal Scale 1 to 1000"),
    renamed_definition(0x40fd19dfd6c99ee4, "Horizontal Scale 1 to 10000"),
    renamed_definition(0xe59988297da41305, "Horizontal Scale 1 to 200"),
    renamed_definition(0xc7f593807dcca30f, "Horizontal Scale 1 to 2000"),
    renamed_definition(0xe5888a297d95a338, "Horizontal Scale 1 to 250"),
    renamed_definition(0x9ca5cc8065440698, "Horizontal Scale 1 to 2500"),
    renamed_definition(0x0550f36d8ea142f8, "Horizontal Scale 1 to 50"),
    renamed_definition(0xa9d06b295c0479d8, "Horizontal Scale 1 to 500"),
    renamed_definition(0x919fff475b9b2538, "Horizontal Scale 1 to 5000"),
    renamed_definition(0x44fd410a36d5505e, "North Point no whiteout"),
    renamed_definition(0xafb836a2812948fb, "North Point with whiteout"),
    renamed_definition(0x6ab0f4901d43b624, "Not For Construction"),
    renamed_definition(0x3c35747b30ec8efc, "SM Basin Label"),
    renamed_definition(0x425550dc0f20e0ee, "SM Pit"),
    renamed_definition(0x56e0127bc52dbcfb, "SURVEY - FU"),
    renamed_definition(0x56e00f7bc52db7e2, "SURVEY - FZ"),
    renamed_definition(0x56f45c7bc53ee3c3, "SURVEY - HO"),
    renamed_definition(0x56f44f7bc53ecdac, "SURVEY - HZ"),
    renamed_definition(0x8d38f77d0bc7ff92, "Site of Work"),
    renamed_definition(0x23dd48a9e9ff2bbe, "Vertical Scale 1 to 100"),
    renamed_definition(0xf02e06b89c97024a, "Vertical Scale 1 to 1000"),
    renamed_definition(0xc98cd081dca080f1, "Vertical Scale 1 to 20"),
    renamed_definition(0x1ac70da9e4bac7f3, "Vertical Scale 1 to 200"),
    renamed_definition(0xc98cd381dca0860a, "Vertical Scale 1 to 25"),
    renamed_definition(0x1ad19fa9e4c4148e, "Vertical Scale 1 to 250"),
    renamed_definition(0xc99dce81dcaef0be, "Vertical Scale 1 to 50"),
    renamed_definition(0x461674a9fd42c14a, "Vertical Scale 1 to 500"),
];

/// The built-in definitions renamed when the built-in customisation was given
/// general names: those whose names began with its publisher's name, which
/// they no longer carry. In the byte order of the names they have now.
pub fn builtin_definition_renames() -> &'static [RenamedDefinition] {
    &BUILTIN_DEFINITION_RENAMES
}

/// For a definition `name` that an exact lookup did not find: the name a
/// rename in builtin_definition_renames gives it now, to look up in its place;
/// empty when no rename knows `name`. Only a lookup that MISSED asks, so a
/// definition that has the earlier name itself, from a person's own library,
/// is still the one drawn.
pub fn definition_name_now(name: &str) -> &'static str {
    definition_name_now_in(name, builtin_definition_renames())
}

/// definition_name_now with the table `renamed`: the C++ two-argument
/// `definitionNameNow`.
pub fn definition_name_now_in(name: &str, renamed: &[RenamedDefinition]) -> &'static str {
    if name.is_empty() {
        return "";
    }
    let hash = source_name_hash(name);
    renamed
        .iter()
        .find(|rename| rename.former_name == hash)
        .map_or("", |rename| rename.now)
}

/// Whether a typed `line` (the verb SAVE and its arguments) can save: with one
/// argument, a project directory, or with none when the drawing already has a
/// project. The arguments are split as the command interpreter splits them -
/// on blanks, double quotes grouping and removed, an unclosed quote refused.
/// A front end writes what it keeps in the metadata - the reference layers -
/// only when this holds: Document::set_metadata marks the drawing modified,
/// and a SAVE that could not go ahead would otherwise leave a drawing nobody
/// touched asking to be saved. The customisation record needs no such guard:
/// the save itself writes it into what it saves (Document::save), and a save
/// that does not go ahead writes nothing.
pub fn typed_save_has_destination(line: &str, has_project: bool) -> bool {
    // CommandInterpreter's word rule (its tokenize), counted rather than kept:
    // blanks split, double quotes group and "" is a word.
    let mut words = 0;
    let mut in_quotes = false;
    let mut in_word = false;
    for byte in line.bytes() {
        if byte == b'"' {
            in_quotes = !in_quotes;
            in_word = true;
        } else if !in_quotes && is_ascii_space(byte) {
            if in_word {
                words += 1;
                in_word = false;
            }
        } else {
            in_word = true;
        }
    }
    if in_quotes {
        return false; // the interpreter refuses the line
    }
    if in_word {
        words += 1;
    }
    // The verb is the first word.
    words == 2 || (words == 1 && has_project)
}
