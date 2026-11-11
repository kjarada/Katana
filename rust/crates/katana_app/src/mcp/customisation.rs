//! katana_customisation (docs/mcp.md, "The customisation"): the session's
//! customisation - its definitions, its survey code rules, what is wrong with
//! them - as structured data for an agent. The port of
//! src/katana_app/mcp_customisation.cpp.
//!
//! Every other tool is a shape over a command line. This one reads the
//! Document instead, as katana_status does, for two reasons. The verbs that
//! list a customisation (CUSTOMISE, CODE LIST, CODE CHECK) reply in text made
//! for a person, and an agent that edits a rule needs the rule, not a line
//! about the code it belongs to. And a read that went through a line would sit
//! in the command history of a session it never changed.
//!
//! Nothing here has words of its own for what a customisation holds. A rule
//! and a definition are written by the Katana customisation format's own
//! writer (katana_entity's customisation) and read back as JSON, so a member
//! here is spelt exactly as a file spells it; the lint and the summary are
//! cad's (code_table, customisation_report). It changes nothing: an edit is
//! CUSTOMISE through katana_run_commands - export, edit the file, load it
//! back.
//!
//! TWO members are the tool's own and no file's: a rule's `index` and a
//! definition's `kind`. A file says the first by the rule's place in "codes"
//! and the second by the list the definition is in, and its reader, which is
//! strict, refuses both as members it does not know. So an entry read here
//! goes into a file with that one member taken off - which the tool's
//! description says, since an agent that copied a rule across whole was
//! refused for it.

use katana_cad::Document;
use katana_cad::code_table::{BuiltInSymbolTest, ColourLookup, LintSeverity, lint_survey_map};
use katana_cad::colour_lookup::colour_lookup;
use katana_cad::customisation_report::customisation_json;
use katana_core::json::Json;
use katana_entity::customisation::{
    Customisation, CustomisationWriteOptions, customisation_to_json, definition_to_json,
};
use katana_entity::entity::to_string;
use katana_entity::style_library::LineStyle;
use katana_entity::tables::is_built_in_symbol_name;

use super::tools::{
    Tool, ToolRefusal, ToolReply, argument, hints, lowered_text, object_schema, property,
    required_string, words,
};
use crate::session::Session;

// How many entries a reply holds when the call does not say, and the most it
// may ask for. A customisation of a survey authority's size has some 1,600
// rules and 800 definitions; a hundred is a screen of them, and a thousand
// keeps one reply to a size a client still reads.
const DEFAULT_LIMIT: usize = 100;
const LARGEST_LIMIT: usize = 1000;

struct Page {
    offset: usize,
    limit: usize,
}

impl Page {
    // Whether the entry that is number `matched` among those the filter took
    // (counted from 0) is on this page.
    fn holds(&self, matched: usize) -> bool {
        matched >= self.offset && matched - self.offset < self.limit
    }
}

// A whole-number argument from `low` to `high`, or `fallback` when it is not
// given.
fn whole_number(
    arguments: &Json,
    name: &str,
    fallback: usize,
    low: i64,
    high: i64,
) -> Result<usize, ToolRefusal> {
    let value = argument(arguments, name);
    if value.is_null() {
        return Ok(fallback);
    }
    if !value.is_number_integer() || value.get::<i64>()? < low || value.get::<i64>()? > high {
        return Err(ToolRefusal::new(format!(
            "\"{name}\" is a whole number from {low}{}",
            if high == i64::MAX {
                String::new()
            } else {
                format!(" to {high}")
            }
        )));
    }
    Ok(value.get::<i64>()? as usize)
}

// An optional text argument; empty when it is not given.
fn text_argument(arguments: &Json, name: &str) -> Result<String, ToolRefusal> {
    let value = argument(arguments, name);
    if value.is_null() {
        return Ok(String::new());
    }
    match value.as_str() {
        Some(text) => Ok(String::from(text)),
        None => Err(ToolRefusal::new(format!("\"{name}\" must be a string"))),
    }
}

// An argument that says nothing for the part asked for is refused, as one the
// tool does not declare is (mcp::server): ignored, a call that filtered the
// summary, or named a definition while asking for the codes, would answer
// something other than what was asked and say nothing of it.
fn refuse_argument(arguments: &Json, name: &str, why: &str) -> Result<(), ToolRefusal> {
    if !argument(arguments, name).is_null() {
        return Err(ToolRefusal::new(format!("\"{name}\" {why}")));
    }
    Ok(())
}

// Whether `text` holds `needle`, which is already in lower case. Searching
// folds case; storing never does (CODE LIST's filter keeps the same rule).
fn holds(text: &str, needle: &str) -> bool {
    needle.is_empty() || lowered_text(text).contains(needle)
}

// The reply every paged part gives: how many entries the filter took, the
// page asked for, and that page under the part's own name.
fn page_of(part: &str, total: usize, page: &Page, entries: Vec<Json>) -> Json {
    Json::object_of([
        ("part", Json::from(part)),
        ("total", Json::from(total)),
        ("offset", Json::from(page.offset)),
        ("limit", Json::from(page.limit)),
        (part, Json::from(entries)),
    ])
}

// ---- summary -------------------------------------------------------------------------------

fn summary_of(document: &Document) -> Result<Json, ToolRefusal> {
    // cad's report, whole, and NOTHING beside it - not the lists' envelope,
    // not even `part`: CUSTOMISE JSON, katana://customisation and this are
    // then one object for one report, and no member cad adds later can meet
    // one of the tool's own under the same name. (A caller knows which part
    // it asked for.) What went wrong when the session started is in it, as
    // `start`: over this protocol there is nowhere else to read that.
    Ok(Json::parse(customisation_json(document))?)
}

// ---- codes ---------------------------------------------------------------------------------

fn codes_of(document: &Document, filter: &str, page: &Page) -> Result<Json, ToolRefusal> {
    let rules = document.survey_map().rules();
    // The rules alone, written as a customisation file writes them: the
    // "codes" list of that text is one object a rule, in map order, with the
    // members the format has and no others. The name is the file's own and is
    // not part of the reply.
    let only_rules = Customisation {
        name: String::from("codes"),
        map: document.survey_map().clone(),
        ..Customisation::default()
    };
    let options = CustomisationWriteOptions {
        linestyles: false,
        symbols: false,
        ..CustomisationWriteOptions::default()
    };
    let text = customisation_to_json(&only_rules, &options).map_err(|error| {
        // What the writer refuses it could not read back. It names the rule.
        ToolRefusal::new(format!(
            "the survey code rules cannot be written as a customisation: {}",
            error.describe()
        ))
    })?;
    let mut file = Json::parse(text)?;
    // A file with no rules has no "codes" member at all.
    let mut written = if file.contains("codes") {
        std::mem::take(&mut file["codes"])
    } else {
        Json::array()
    };
    if written.size() != rules.len() {
        return Err(ToolRefusal::new(format!(
            "the survey code rules were not all written: {} of {}",
            written.size(),
            rules.len()
        )));
    }

    let needle = lowered_text(filter);
    let mut entries = Vec::new();
    let mut matches = 0usize;
    for (index, rule) in rules.iter().enumerate() {
        if !holds(&rule.key, &needle)
            && !holds(&rule.comment, &needle)
            && !holds(&rule.model, &needle)
        {
            continue;
        }
        if page.holds(matches) {
            let mut entry = std::mem::take(&mut written[index]);
            // The only identity a rule has: its place in the map, which is
            // what CODE EXPLAIN and CODE CHECK cite it by ("rule #57").
            entry["index"] = Json::from(index);
            entries.push(entry);
        }
        matches += 1;
    }
    Ok(page_of("codes", matches, page, entries))
}

// ---- definitions ---------------------------------------------------------------------------

// One definition: what a list shows of it, and with `whole` everything a
// customisation file holds for it, its strokes included.
fn definition_of(definition: &LineStyle, whole: bool) -> Result<Json, ToolRefusal> {
    let text = definition_to_json(definition, "").map_err(|error| {
        ToolRefusal::new(format!(
            "the definition cannot be written as a customisation's: {}",
            error.describe()
        ))
    })?;
    let mut written = Json::parse(text)?;
    // The format leaves a member at its default out; a list says each of
    // these for every definition, so that a reader need not know the
    // defaults. "world" is the one the format gives `units` left out
    // (docs/customisation.md, "The members"); the others are the model's.
    let entry = Json::object_of([
        ("name", Json::from(&definition.name)),
        (
            "kind",
            Json::from(if definition.symbol {
                "symbol"
            } else {
                "linestyle"
            }),
        ),
        ("group", Json::from(&definition.group)),
        (
            "units",
            Json::from(written.value("units", String::from("world"))?),
        ),
        ("atVertices", Json::from(definition.at_vertices)),
        ("from", Json::from(&definition.source)),
    ]);
    if !whole {
        return Ok(entry);
    }
    if !written.contains("strokes") {
        // A definition with no strokes has no such member in a file; asked
        // for by name, it says that it has none.
        written["strokes"] = Json::array();
    }
    // nlohmann's update(): each member of the entry put in, replacing one of
    // its name.
    for (key, value) in entry.items() {
        written[&*key] = value.clone();
    }
    Ok(written)
}

fn definitions_of(
    document: &Document,
    filter: &str,
    name: &str,
    page: &Page,
) -> Result<Json, ToolRefusal> {
    let library = document.style_library();
    if !name.is_empty() {
        // Matched as written, as a rule's linestyle or symbol names it.
        let Some(definition) = library.find(name) else {
            return Err(ToolRefusal::new(format!(
                "no definition is called \"{name}\"; names are matched as written, letter case \
                 included (part definitions with a filter lists them)"
            )));
        };
        return Ok(page_of(
            "definitions",
            1,
            &Page {
                offset: 0,
                limit: 1,
            },
            vec![definition_of(definition, true)?],
        ));
    }
    let needle = lowered_text(filter);
    let mut entries = Vec::new();
    let mut matches = 0usize;
    // The C++ throws out of its forEach; here the first refusal ends the walk.
    let mut refused: Option<ToolRefusal> = None;
    library.for_each(|definition| {
        if refused.is_some()
            || (!holds(&definition.name, &needle) && !holds(&definition.group, &needle))
        {
            return;
        }
        if page.holds(matches) {
            match definition_of(definition, false) {
                Ok(entry) => entries.push(entry),
                Err(refusal) => {
                    refused = Some(refusal);
                    return;
                }
            }
        }
        matches += 1;
    });
    if let Some(refusal) = refused {
        return Err(refusal);
    }
    Ok(page_of("definitions", matches, page, entries))
}

// ---- problems ------------------------------------------------------------------------------

fn problems_of(document: &Document, filter: &str, page: &Page) -> Json {
    // The lint CODE CHECK prints, an object an issue.
    let resolve = colour_lookup(document);
    let colour_of: &ColourLookup<'_> = &resolve;
    let built_in: &BuiltInSymbolTest<'_> = &is_built_in_symbol_name;
    let issues = lint_survey_map(
        document.survey_map(),
        document.style_library(),
        Some(colour_of),
        Some(built_in),
    );
    let needle = lowered_text(filter);
    let mut entries = Vec::new();
    let mut matches = 0usize;
    let mut cannot_apply = 0usize;
    for issue in &issues {
        cannot_apply += usize::from(issue.severity == LintSeverity::Error);
        let kind = to_string(issue.kind);
        if !holds(&issue.key, &needle) && !holds(kind, &needle) && !holds(&issue.message, &needle) {
            continue;
        }
        if page.holds(matches) {
            // `index` is the rule's, the `index` part codes gives it.
            entries.push(Json::object_of([
                ("index", Json::from(issue.rule)),
                ("key", Json::from(&issue.key)),
                ("sets", Json::from(to_string(issue.section))),
                ("severity", Json::from(to_string(issue.severity))),
                ("kind", Json::from(kind)),
                ("message", Json::from(&issue.message)),
            ]));
        }
        matches += 1;
    }
    let mut reply = page_of("problems", matches, page, entries);
    // Of the whole lint, whatever the filter took: the three numbers the
    // summary's "problems" gives, by the same names.
    reply["rules"] = Json::from(document.survey_map().size());
    reply["cannotApply"] = Json::from(cannot_apply);
    reply["warnings"] = Json::from(issues.len() - cannot_apply);
    reply
}

fn customisation(session: &mut Session, arguments: &Json) -> Result<ToolReply, ToolRefusal> {
    let document = session.document();
    let part = required_string(arguments, "part")?;
    let structured = if part == "summary" {
        for name in ["filter", "name", "offset", "limit"] {
            refuse_argument(
                arguments,
                name,
                "does not go with part \"summary\", which is one object and not a list",
            )?;
        }
        summary_of(document)?
    } else if part == "codes" || part == "definitions" || part == "problems" {
        let name = text_argument(arguments, "name")?;
        if part != "definitions" {
            refuse_argument(
                arguments,
                "name",
                "picks one definition: it goes with part \"definitions\"",
            )?;
        } else if !argument(arguments, "name").is_null() {
            if name.is_empty() {
                return Err(ToolRefusal::new("\"name\" must be a non-empty string"));
            }
            for other in ["filter", "offset", "limit"] {
                refuse_argument(
                    arguments,
                    other,
                    "does not go with \"name\", which picks exactly one definition",
                )?;
            }
        }
        let filter = text_argument(arguments, "filter")?;
        let page = Page {
            offset: whole_number(arguments, "offset", 0, 0, i64::MAX)?,
            limit: whole_number(arguments, "limit", DEFAULT_LIMIT, 1, LARGEST_LIMIT as i64)?,
        };
        match part.as_str() {
            "codes" => codes_of(document, &filter, &page)?,
            "definitions" => definitions_of(document, &filter, &name, &page)?,
            _ => problems_of(document, &filter, &page),
        }
    } else {
        return Err(ToolRefusal::new(
            "\"part\" is summary, codes, definitions or problems",
        ));
    };
    // The text is the same object, for a client older than structured
    // content: it has every fact, as katana_status' text has. (A name that is
    // not UTF-8 was shown with U+FFFD in the C++; a Rust String holds none.)
    Ok(ToolReply {
        text: structured.dump_with(2, ' ', false),
        structured,
        is_error: false,
    })
}

/// katana_customisation: the session's customisation as structured data -
/// what is loaded, its survey code rules, its definitions and the rules'
/// lint. Read from the Document.
pub(crate) fn customisation_tool() -> Tool {
    Tool {
        name: "katana_customisation",
        title: "The customisation",
        description: concat!(
            "The session's customisation as structured data: the linestyle and symbol definitions, ",
            "the survey code rules, the colours and the settings that turn a surveyor's field codes ",
            "into a drawing. part chooses what: summary - what is loaded (name, origin, each source ",
            "and its notice, counts, linework codes, automation, colours, builtIns: the built-in ",
            "customisations the program has, with the default and the one this session is - ",
            "CUSTOMISE BUILTIN <name> through katana_run_commands chooses one - what is wrong with the ",
            "rules, what this drawing uses of it, and start: what went wrong when this session ",
            "started, such as a kept customisation that did not read); it is ONE object, exactly ",
            "what CUSTOMISE JSON prints and the katana://customisation resource holds, with no ",
            "part, total, offset or limit beside its members; ",
            "codes - one object a survey code rule: its index (its place, which is its precedence ",
            "and what CODE EXPLAIN and CODE CHECK cite it by), key, sets and the members a Katana ",
            "customisation file holds for it; definitions - one object a linestyle or symbol: name, ",
            "kind, group, units, atVertices, from - and with name, that one definition whole, its ",
            "strokes included; problems - the rules' lint: index, key, sets, severity, kind, ",
            "message. codes, definitions and problems are lists, each {part, total, offset, limit, ",
            "<part>: [...]}: total is how many entries filter ",
            "took, and offset and limit (100 unless said, at most 1000) choose the page. It reads ",
            "only. To CHANGE a customisation use katana_run_commands: CUSTOMISE EXPORT <file> ",
            "writes the session as one Katana customisation file (JSON), which you edit, and ",
            "CUSTOMISE <file> merges a file in - a definition replaces the one of its name, a ",
            "key's rules in a section replace that key's there, so a file holding only what ",
            "changes is an edit - or CUSTOMISE REPLACE <file> loads it in the place of what is ",
            "loaded; CUSTOMISE REMOVE deletes, CUSTOMISE SET sets the switches and linework codes ",
            "(HELP CUSTOMISE). A rule, or a definition read whole by name, goes into such a file ",
            "once the member that is this tool's own is taken off it - index from a rule, kind ",
            "from a definition: a file says a rule's place by its order in codes and a ",
            "definition's kind by the list it is in, linestyles or symbols, and refuses a member ",
            "it does not know. An entry of the definitions LIST is not the definition: it has no ",
            "strokes."
        ),
        input_schema: object_schema(
            Json::object_of([
                (
                    "part",
                    property(
                        Json::from("string"),
                        "summary: one object, what is loaded and what went wrong at the start (no \
                         part or total beside it). codes: the survey code rules. definitions: the \
                         linestyles and symbols. problems: what the lint finds in the rules.",
                        [(
                            "enum",
                            words(["summary", "codes", "definitions", "problems"]),
                        )],
                    ),
                ),
                (
                    "filter",
                    property(
                        Json::from("string"),
                        "Only the entries holding this text, letter case ignored: in a rule's \
                         key, comment or layer (codes); a definition's name or group \
                         (definitions); an issue's key, kind or message (problems).",
                        [],
                    ),
                ),
                (
                    "name",
                    property(
                        Json::from("string"),
                        "With part definitions: exactly one definition, by its name as written, \
                         with everything a customisation file holds for it - its strokes too.",
                        [],
                    ),
                ),
                (
                    "offset",
                    property(
                        Json::from("integer"),
                        "How many matching entries to skip (default 0).",
                        [("minimum", Json::from(0))],
                    ),
                ),
                (
                    "limit",
                    property(
                        Json::from("integer"),
                        "The most entries to return (default 100).",
                        [
                            ("minimum", Json::from(1)),
                            ("maximum", Json::from(LARGEST_LIMIT)),
                        ],
                    ),
                ),
            ]),
            &["part"],
        ),
        annotations: hints(true, false, true, false),
        call: customisation,
    }
}
