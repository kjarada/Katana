//! What a PART of a customisation is written as: its codes for a colleague,
//! two of its symbols, its linestyles without its rules
//! (include/katana/cad/customisation_part.hpp).
//!
//! One rule, for everything that writes less than the whole: CUSTOMISE EXPORT
//! with a kind word or ONLY (customisation_verbs), the Survey Code Manager's
//! Export Codes and the Symbol Library's Export Selected. The verb and the two
//! managers were written side by side and cut a session down by two rules
//! that disagreed - about the colours, about whose notice travels and about
//! what a part is based on - so the same symbols exported from the window and
//! from the command line were two different files.
//!
//! ```text
//! name, description,  the customisation's own, as they are. A notice is
//! notice              "carried with the data and shown to whoever uses it"
//!                     (entity/customisation), and a part of a
//!                     customisation is its author's data too.
//! sources             those that brought what is WRITTEN, each said to have
//!                     brought that alone, with its own notice:
//!                       definitions  a written definition came from it
//!                                    (its LineStyle::source)
//!                       rules        the rules are written, and it brought
//!                                    some
//!                       neither      it brought neither kind - a table of
//!                                    colours - and the part carries a colour
//!                     A file's sources are taken at their word where it is
//!                     loaded: each becomes a source of that session BY NAME,
//!                     and a project's warning that a customisation is
//!                     missing goes by name too. One listed with nothing of
//!                     it written would pass for loaded.
//! the notice of a     written with the part's own, after it, each line
//! source left out     once. Nothing says whose a colour is, and an author's
//!                     notice is never dropped by an export.
//! colours             those the written rules and the written definitions'
//!                     pens NAME - a rule's colour, its symbol's and its
//!                     text's; a pen stroke - compared as colour names are
//!                     (entity::fold_colour_name). Without them the part would
//!                     draw elsewhere in other colours than here; the rest
//!                     would overwrite colours of whoever loads it that
//!                     nothing in the file uses.
//! linework,           absent: a part says nothing of them, so loading it
//! automation          leaves the session's own alone. "A file of symbols for
//!                     a colleague must not reset their control codes"
//!                     (entity/customisation), nor a file of codes.
//! based_on            absent: a part is not an edition of the built-in, to
//!                     be told from another edition at the next start.
//! ```
//!
//! The WHOLE customisation is not a part and is not asked of this: an export
//! with no kind word, and CUSTOMISE KEEP, write everything as it is.

use std::collections::BTreeSet;

use katana_entity::colour_names::fold_colour_name;
use katana_entity::{ColourTable, Customisation, CustomisationWriteOptions, StrokeOp, SurveyMap};

// A colour name something written names, by its fold: the identity a colour
// table finds it by.
fn names(named: &mut BTreeSet<String>, colour: &str) {
    if !colour.is_empty() {
        named.insert(fold_colour_name(colour));
    }
}

/// `session` cut down to the part a write with `written` holds, to be handed
/// to entity's customisation_to_json WITH THE SAME `written`.
///
/// The definitions and the rules themselves are left whole: the writer
/// chooses among them by `written` and refuses a name it cannot write (one
/// the library lacks, or one whose kind is not being written), and what is
/// counted here as written is chosen by the same rule - a definition of a
/// kind `written` names and, when `written.only` is not empty, of a name in
/// it; the rules when `written.codes`. A name `only` gives that the writer
/// would refuse is simply not counted.
///
/// It reads nothing but the two values, so a session edited in a buffer -
/// the Survey Code Manager's rules, not yet applied - is a part like any
/// other: the caller puts the buffer in `session.map` first.
pub fn customisation_part(
    session: Customisation,
    written: &CustomisationWriteOptions,
) -> Customisation {
    let mut part = session;
    part.linework = None;
    part.automation = None;
    part.based_on = None;

    // What the write will hold, chosen as the writer chooses (entity's
    // customisation_to_json): where each written definition came from, and
    // the colours the written definitions and rules name, by the fold a
    // colour name is found by.
    let mut written_from: BTreeSet<String> = BTreeSet::new();
    let mut named: BTreeSet<String> = BTreeSet::new();
    let only: BTreeSet<&str> = written.only.iter().map(String::as_str).collect();
    part.library.for_each(|definition| {
        let kind = if definition.symbol {
            written.symbols
        } else {
            written.linestyles
        };
        if !kind || (!only.is_empty() && !only.contains(definition.name.as_str())) {
            return;
        }
        written_from.insert(definition.source.clone());
        for stroke in &definition.strokes {
            if stroke.op == StrokeOp::Pen {
                names(&mut named, &stroke.pen);
            }
        }
    });
    let rules_written = written.codes && !part.map.empty();
    if rules_written {
        // The three places a rule names a colour (the lint's own three,
        // lint_survey_rule).
        for rule in part.map.rules() {
            names(&mut named, &rule.colour);
            if let Some(symbol) = &rule.symbol {
                names(&mut named, &symbol.colour);
            }
            if let Some(text_style) = &rule.text_style {
                names(&mut named, &text_style.colour);
            }
        }
    }
    let mut carried = ColourTable::new();
    for colour in part.colours.entries() {
        if named.contains(&fold_colour_name(&colour.name)) {
            // Cannot be refused: the entry comes out of a table that took it.
            let _ = carried.add(colour.name, colour.colour);
        }
    }
    part.colours = carried;

    let sources = std::mem::take(&mut part.sources);
    let mut held = Vec::with_capacity(sources.len());
    for mut source in sources {
        let brought_neither = !source.definitions && !source.rules;
        source.definitions = written_from.contains(&source.name);
        source.rules = source.rules && rules_written;
        if source.definitions || source.rules || (brought_neither && !part.colours.empty()) {
            held.push(source);
            continue;
        }
        for line in source.notice {
            if !part.notice.contains(&line) {
                part.notice.push(line);
            }
        }
    }
    part.sources = held;
    part
}

/// What a load of DEFINITIONS alone (CUSTOMISE DEFINITIONS, which the Symbol
/// Library's Import Definitions runs) takes of a file: its definitions and
/// its colours. Its survey code rules are the Survey Code Manager's to
/// review; its control codes and switches are settings of whoever wrote the
/// file.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DefinitionsOfFile {
    pub taken: Customisation,
    /// What was left out, counted for the reply: the rules, and whether the
    /// file said anything of the linework codes or the automation.
    pub rules_left: usize,
    pub linework_left: bool,
    pub automation_left: bool,
}

/// `file` without its rules, linework codes and automation - and without the
/// sources that brought it only rules. A source it lists becomes one of the
/// session's, by name, and is taken off what the open project is missing
/// (Document::install_customisation): one left in with its rules flag cleared
/// would pass for loaded with none of its rules here.
///
/// ```text
/// a source that brought definitions     kept, and said to have brought those
///                                       ALONE: its rules are not taken
/// one that brought rules alone          dropped, its notice with it
/// one that brought neither (a table     kept only when the file has colours,
/// of colours)                           which the load takes
/// ```
///
/// It goes by what the file SAYS each source brought - the load takes every
/// definition of the file - where a part being WRITTEN goes by where each
/// written definition came from (customisation_part).
pub fn definitions_of_file(file: Customisation) -> DefinitionsOfFile {
    let mut file = file;
    let rules_left = file.map.size();
    let linework_left = file.linework.is_some();
    let automation_left = file.automation.is_some();
    file.map = SurveyMap::default();
    file.linework = None;
    file.automation = None;
    let with_colours = !file.colours.empty();
    let sources = std::mem::take(&mut file.sources);
    let mut standing = Vec::with_capacity(sources.len());
    for mut source in sources {
        let neither = !source.definitions && !source.rules;
        if !source.definitions && !(neither && with_colours) {
            continue;
        }
        source.rules = false;
        standing.push(source);
    }
    file.sources = standing;
    DefinitionsOfFile {
        taken: file,
        rules_left,
        linework_left,
        automation_left,
    }
}
