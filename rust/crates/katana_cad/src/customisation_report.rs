//! What customisation a session has: the counts, where it came from, the
//! sources that went into it, what the open project was drawn with that is
//! not loaded, and what it covers in THIS drawing. One result and its
//! formatters, as code_table has them, so every front end says the same words
//! (include/katana/cad/customisation_report.hpp,
//! src/katana_cad/customisation/customisation_report.cpp).
//!
//! The text is format_customisation_reply: what the bare CUSTOMISE of the
//! shared verb (customisation_verbs) replies and File > Drawing Summary shows -
//! the two count lines, then key=value records, then the coverage.
//! customisation_json is the same as one JSON object, for CUSTOMISE JSON and
//! for a front end that hands a client structured data.
//!
//! The form of customisation_summary that takes the sources and the missing
//! names as arguments (customisation_summary_of) describes LISTS, so that the
//! numbers can be tested without loading anything; the form that takes the
//! Document alone reads them from its state (customisation_state), where they
//! live.

use std::collections::BTreeMap;

use katana_core::json::Json;
use katana_entity::linework_codes::linework_code_members;
use katana_entity::style_library::style_groups;
use katana_entity::tables::is_built_in_symbol_name;
use katana_entity::{CustomisationAutomation, EnumName, LineworkCodes};

use crate::code_table::{
    BuiltInSymbolTest, ColourLookup, LintSeverity, format_coverage, lint_survey_map,
};
use crate::colour_lookup::colour_lookup;
use crate::customisation_host::{BuiltInEntry, built_in_choices, built_ins};
use crate::customisation_record::CustomisationSource;
use crate::customisation_state::CustomisationOrigin;
use crate::document::Document;
use crate::scope_verbs::record_value;
use crate::style_catalogue::{
    CatalogueEntry, DefinitionSource, linetype_choices, symbol_definition_count,
};
use crate::survey_coding::{CustomisationCoverage, customisation_coverage, is_plain_linestyle};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CustomisationSummary {
    /// In the style library.
    pub definitions: usize,
    /// entity's style_groups.
    pub groups: usize,
    /// Library definitions the pickers offer as each (decision D3,
    /// symbol_choices and linetype_choices); one definition can be both.
    pub symbols: usize,
    pub linestyles: usize,
    /// Survey code rules.
    pub rules: usize,
    /// Distinct keys among them.
    pub codes: usize,
    pub loaded: Vec<CustomisationSource>,
    pub not_loaded: Vec<String>,
    pub coverage: CustomisationCoverage,
    /// The session's own state. The form of customisation_summary that is
    /// handed the two lists above leaves these as they are here: it describes
    /// lists, not a session.
    pub name: String,
    pub origin: CustomisationOrigin,
    pub kept: bool,
    /// Names in the customisation's own colour table.
    pub colours: usize,
    pub automation: CustomisationAutomation,
    pub linework: LineworkCodes,
}

// Library definitions only: the pickers also offer the model's linetypes and
// the built-in shapes, which no customisation brought. This line once said
// "157 of them symbols", counting `mode vertex` alone - one of D3's four
// reasons, and a minority of the symbols the reference survey code files use.
fn from_library(entries: &[CatalogueEntry]) -> usize {
    entries
        .iter()
        .filter(|entry| entry.source == DefinitionSource::Library)
        .count()
}

fn counted(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

// Nothing to count: no definition and no rule. (A session may still hold a
// name, colours and settings; it is the definitions and the rules that draw.)
fn nothing_loaded(summary: &CustomisationSummary) -> bool {
    summary.definitions == 0 && summary.rules == 0
}

// The two lines the reply begins with when something is loaded.
fn write_counts(out: &mut String, summary: &CustomisationSummary) {
    out.push_str(&format!(
        "{} in {}: {} offered as symbols, {} as linestyles (one definition can be both)\n{} \
         over {}\n",
        counted(
            summary.definitions,
            "linestyle and symbol definition",
            "linestyle and symbol definitions"
        ),
        counted(summary.groups, "group", "groups"),
        summary.symbols,
        summary.linestyles,
        counted(summary.rules, "survey code rule", "survey code rules"),
        counted(summary.codes, "distinct code", "distinct codes")
    ));
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}

// A list of names as nlohmann writes a std::vector<std::string>.
fn texts(items: &[String]) -> Json {
    Json::array_of(items.iter().map(|item| Json::from(item.as_str())))
}

/// The C++'s customisationSummary(document, loaded, missingAtOpen): the
/// numbers of `document`'s library, map and coverage, with `loaded` and
/// `missing_at_open` as the lists it describes.
pub fn customisation_summary_of(
    document: &Document,
    loaded: &[CustomisationSource],
    missing_at_open: &[String],
) -> CustomisationSummary {
    let library = document.style_library();
    CustomisationSummary {
        definitions: library.size(),
        groups: style_groups(library).len(),
        symbols: symbol_definition_count(document),
        linestyles: from_library(&linetype_choices(document, false)),
        rules: document.survey_map().size(),
        codes: document.survey_map().keys().len(),
        loaded: loaded.to_vec(),
        not_loaded: missing_at_open.to_vec(),
        coverage: customisation_coverage(document),
        ..CustomisationSummary::default()
    }
}

// ---- the verb's reply -------------------------------------------------------------------

/// The session as the Document holds it: its sources, what the open project
/// is missing, and its name, origin, kept flag, colours, automation and
/// linework codes.
pub fn customisation_summary(document: &Document) -> CustomisationSummary {
    let state = document.customisation_state();
    let mut summary = customisation_summary_of(document, &state.sources, &state.missing_at_open);
    summary.name.clone_from(&state.name);
    summary.origin = state.origin;
    summary.kept = state.kept;
    summary.colours = state.colours.size();
    summary.automation = state.automation;
    summary.linework = state.linework.clone();
    summary
}

/// The first of the reply's records alone, with no line break: what a verb
/// that changed the session ends its reply with, so the reply says what is
/// loaded now.
pub fn customisation_state_record(summary: &CustomisationSummary) -> String {
    // The counts again, although the two lines above them say them in words:
    // a script reads this line and should not have to read a sentence.
    format!(
        "customisation name={} origin={} kept={} definitions={} codes={} rules={} colours={}",
        record_value(&summary.name),
        summary.origin.name(),
        yes_no(summary.kept),
        summary.definitions,
        summary.codes,
        summary.rules,
        summary.colours
    )
}

/// Lines ending in '\n': the two count lines - definitions by what they are
/// offered as, rules over distinct codes - (or that nothing is loaded and how
/// to load, with no coverage to report of nothing; of a customisation that
/// brought neither kind - colours, settings - that no definitions and no
/// rules are loaded, never that no customisation is), then one record a line
///
/// ```text
///   customisation name=NSW origin=builtIn kept=yes definitions=792 codes=632 rules=1624 colours=12
///   source name=NSW definitions=yes rules=yes               one a source, in load order
///   automation auto.codes=on auto.linework=on
///   linework linework.start=ST linework.end=END ... linework.rectangle=RECT
///   missing name="old symbols"                              one a name the project
///                                                           recorded and the session lacks
/// ```
///
/// then the coverage lines, when anything is loaded. The automation and
/// linework records carry the very keys CUSTOMISE SET takes, so either can be
/// typed back after it. A record's values are written by record_value
/// (scope_verbs).
pub fn format_customisation_reply(summary: &CustomisationSummary) -> String {
    let mut out = String::new();
    let nothing = nothing_loaded(summary);
    if nothing {
        // A session may hold a customisation that brought neither kind - a
        // name, a table of colours, the settings. "No customisation is
        // loaded." above the record that names it and gives its origin
        // contradicted the next line; what is true of it is that nothing in
        // it draws.
        out.push_str(if summary.origin == CustomisationOrigin::None {
            "No customisation is loaded.\n"
        } else {
            "No linestyle or symbol definitions and no survey code rules are loaded.\n"
        });
        out.push_str("  CUSTOMISE <file> [<file>...]  loads Katana customisation files\n");
    } else {
        write_counts(&mut out, summary);
    }
    out.push_str(&customisation_state_record(summary));
    out.push('\n');
    for source in &summary.loaded {
        out.push_str(&format!(
            "source name={} definitions={} rules={}\n",
            record_value(&source.name),
            yes_no(source.definitions),
            yes_no(source.rules)
        ));
    }
    // The keys CUSTOMISE SET takes, so that either record can be typed back
    // after it. A control switched off has no spelling, written "".
    out.push_str(&format!(
        "automation auto.codes={} auto.linework={}\n",
        on_off(summary.automation.codes_on_survey_import),
        on_off(summary.automation.linework_on_survey_import)
    ));
    out.push_str("linework");
    for member in linework_code_members() {
        out.push_str(&format!(
            " linework.{}={}",
            member.name.to_ascii_lowercase(),
            record_value(member.spelling(&summary.linework))
        ));
    }
    out.push('\n');
    // What the OPEN warned of, kept in view: said once at the open, it
    // scrolled away while what those names defined still drew as plain lines.
    for name in &summary.not_loaded {
        out.push_str(&format!("missing name={}\n", record_value(name)));
    }
    if !nothing {
        out.push_str(&format_coverage(&summary.coverage));
    }
    out
}

/// The linestyle and symbol names the survey code rules ask for that nothing
/// defines: not a definition of the library, not a plain line, not a symbol
/// Katana draws itself. Distinct, in name order. A customisation need not be
/// self-contained, and saying which names are missing is the difference
/// between a symbol that is plainly absent and one silently drawn as a dot.
pub fn undefined_rule_names(document: &Document) -> Vec<String> {
    document
        .survey_map()
        .styles_referenced()
        .into_iter()
        .filter(|name| {
            !is_plain_linestyle(name)
                && document.definition_for(name).is_none()
                && !is_built_in_symbol_name(name)
        })
        .collect()
}

/// The session's customisation as ONE JSON object - CUSTOMISE JSON, and what
/// a front end gives a client that wants structure (katana_mcp's resource).
/// Its keys are in alphabetical order, two blanks a level, with no line break
/// at the end:
///
/// ```text
///   name, origin, kept, description, notice, basedOn (null when none), builtIn
///   (the name of the host's built-in, "" when it has none)
///   builtIns     [{name, description, source, default, active, definitions,
///                 codes, rules[, problem]}] - the built-in customisations the
///                program offers (built_in_choices), the order CUSTOMISE BUILTIN
///                lists them in: `default` marks the one `builtIn` names,
///                `active` the one the session is, as installed; `source` is
///                the file it was compiled in from, "" for one that was not;
///                `problem` is there only for one that did not read
///   sources      [{name, definitions, rules, notice}], in load order
///   counts       {definitions, groups, symbols, linestyles, rules, codes, colours}
///   automation   {codesOnSurveyImport, lineworkOnSurveyImport}
///   linework     {start, end, close, arcStart, arcEnd, join, rectangle}
///   colours      {name: "#RRGGBB"}, the customisation's own table
///   problems     {rules, cannotApply, warnings, byKind: {kind: count},
///                 undefined: [name]} - the lint CODE CHECK prints, counted
///                (cannotApply is its count of rules that cannot be applied as
///                written), and undefined_rule_names
///   coverage     {styles, named, resolved, builtIn, unresolved, notLinestyles}
///   missing      [name], what the open project recorded and the session lacks
///   start        {problems: [sentence], keptFromAnotherBuiltIn} - what
///                start_customisation found when the session started
///                (CustomisationState::start_problems): a kept file or a
///                built-in that did not read, and a kept customisation made
///                from another built-in than the program has. Always there,
///                [] and false when the start had nothing to say; it is of the
///                START, and nothing done since changes it
/// ```
///
/// automation, linework and basedOn use the member names of the Katana
/// customisation format, so the report reads against a file. It does not hold
/// the definitions or the rules themselves: a file does (CUSTOMISE EXPORT).
/// The C++ wrote text that is not UTF-8 with U+FFFD rather than throwing; a
/// String holds none. The built-ins it lists are those compiled into the
/// program (built_ins()).
pub fn customisation_json(document: &Document) -> String {
    customisation_json_offering(document, built_ins())
}

/// The same, with `offered` as the built-ins it lists: what CUSTOMISE JSON
/// gives (built_ins_of its host), where the form above lists those compiled
/// into the program - which is the same list, unless the host was given
/// another to offer, as a test's is. The form a katana_mcp client is given.
/// (The C++'s second overload of customisationJson.)
pub fn customisation_json_offering(document: &Document, offered: &[BuiltInEntry]) -> String {
    let state = document.customisation_state();
    let summary = customisation_summary(document);

    let sources = Json::array_of(state.sources.iter().map(|source| {
        Json::object_of([
            ("name", Json::from(source.name.as_str())),
            ("definitions", Json::from(source.definitions)),
            ("rules", Json::from(source.rules)),
            ("notice", texts(&source.notice)),
        ])
    }));
    let mut colours = Json::object();
    for entry in state.colours.entries() {
        colours[entry.name.as_str()] = Json::from(entry.colour.to_hex());
    }
    let mut linework = Json::object();
    for member in linework_code_members() {
        linework[member.name] = Json::from(member.spelling(&state.linework));
    }

    // The lint CODE CHECK prints, counted. A count of the rules that cannot
    // be applied is called that, and never by the lint's own word for them:
    // a reply that succeeded must not hold a word a script takes for failure.
    let resolve = colour_lookup(document);
    let colour_of: &ColourLookup<'_> = &resolve;
    let built_in: &BuiltInSymbolTest<'_> = &is_built_in_symbol_name;
    let issues = lint_survey_map(
        document.survey_map(),
        document.style_library(),
        Some(colour_of),
        Some(built_in),
    );
    let mut cannot_apply = 0usize;
    let mut by_kind: BTreeMap<&str, usize> = BTreeMap::new();
    for issue in &issues {
        cannot_apply += usize::from(issue.severity == LintSeverity::Error);
        *by_kind.entry(issue.kind.name()).or_default() += 1;
    }

    let built_in_list = Json::array_of(built_in_choices(document, offered).into_iter().map(
        |choice| {
            let mut one = Json::object_of([
                ("name", Json::from(choice.name.as_str())),
                ("description", Json::from(choice.description.as_str())),
                ("source", Json::from(choice.source.as_str())),
                ("default", Json::from(choice.is_default)),
                ("active", Json::from(choice.is_active)),
                ("definitions", Json::from(choice.definitions)),
                ("codes", Json::from(choice.codes)),
                ("rules", Json::from(choice.rules)),
            ]);
            if !choice.problem.is_empty() {
                one["problem"] = Json::from(choice.problem.as_str());
            }
            one
        },
    ));

    let based_on = match &state.based_on {
        Some(base) => Json::object_of([
            ("name", Json::from(base.name.as_str())),
            ("digest", Json::from(base.digest.as_str())),
        ]),
        None => Json::Null,
    };
    let json = Json::object_of([
        ("name", Json::from(state.name.as_str())),
        ("origin", Json::from(state.origin.name())),
        ("kept", Json::from(state.kept)),
        ("description", Json::from(state.description.as_str())),
        ("notice", texts(&state.notice)),
        ("basedOn", based_on),
        ("builtIn", Json::from(state.built_in.as_str())),
        ("builtIns", built_in_list),
        ("sources", sources),
        (
            "counts",
            Json::object_of([
                ("definitions", Json::from(summary.definitions)),
                ("groups", Json::from(summary.groups)),
                ("symbols", Json::from(summary.symbols)),
                ("linestyles", Json::from(summary.linestyles)),
                ("rules", Json::from(summary.rules)),
                ("codes", Json::from(summary.codes)),
                ("colours", Json::from(summary.colours)),
            ]),
        ),
        (
            "automation",
            Json::object_of([
                (
                    "codesOnSurveyImport",
                    Json::from(state.automation.codes_on_survey_import),
                ),
                (
                    "lineworkOnSurveyImport",
                    Json::from(state.automation.linework_on_survey_import),
                ),
            ]),
        ),
        ("linework", linework),
        ("colours", colours),
        (
            "problems",
            Json::object_of([
                ("rules", Json::from(summary.rules)),
                ("cannotApply", Json::from(cannot_apply)),
                ("warnings", Json::from(issues.len() - cannot_apply)),
                (
                    "byKind",
                    Json::object_of(
                        by_kind
                            .iter()
                            .map(|(kind, count)| (*kind, Json::from(*count))),
                    ),
                ),
                ("undefined", texts(&undefined_rule_names(document))),
            ]),
        ),
        (
            "coverage",
            Json::object_of([
                ("styles", Json::from(summary.coverage.styles)),
                ("named", Json::from(summary.coverage.named)),
                ("resolved", Json::from(summary.coverage.resolved)),
                ("builtIn", Json::from(summary.coverage.built_in)),
                ("unresolved", texts(&summary.coverage.unresolved)),
                ("notLinestyles", texts(&summary.coverage.not_linestyles)),
            ]),
        ),
        ("missing", texts(&state.missing_at_open)),
        // What the start found, always written - empty when it had nothing
        // to say - so that a reader can tell "nothing went wrong" from a
        // build that does not say.
        (
            "start",
            Json::object_of([
                ("problems", texts(&state.start_problems)),
                (
                    "keptFromAnotherBuiltIn",
                    Json::from(state.kept_from_another_built_in),
                ),
            ]),
        ),
    ]);
    json.dump_with(2, ' ', false)
}
