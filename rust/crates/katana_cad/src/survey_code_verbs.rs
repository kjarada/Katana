//! CODE: the survey codes on the text command line (docs/survey_coding.md;
//! include/katana/cad/survey_code_verbs.hpp,
//! src/katana_cad/customisation/survey_code_verbs.cpp). The interpreter's, so
//! the window's command line, katana_cli, katana_mcp and an AI agent all run
//! the same verb.
//!
//! ```text
//!   CODE [<scope>] [WHERE ...] [PROPERTY <name>] [PREVIEW]
//!                               apply the loaded survey codes to the entities
//!                               the scope takes that carry a field code, as
//!                               ONE undo step; PREVIEW changes nothing
//!   CODE CENSUS [<scope>] [WHERE ...] [PROPERTY <name>] [PREVIEW]
//!                               the codes those entities carry (it changes
//!                               nothing, so PREVIEW asks nothing more of it:
//!                               the word is taken so that the two forms are
//!                               one grammar)
//!   CODE EXPLAIN <code>         why a code gets what it gets
//!   CODE LIST [<filter>]        the loaded survey codes, one code per line
//!   CODE CHECK                  lint them; refused when a rule has an error
//! ```
//!
//! THE SCOPE is the one every verb on drawing data takes (scope_verbs), read
//! by the one parser. What differs here is the DEFAULT: with no scope word
//! CODE takes the whole drawing, and under a bare WHERE too, where MODIFY and
//! the rest take the selection. CODE was the whole drawing before it took a
//! scope, and a script that says CODE must not come to mean "whatever happens
//! to be selected".
//!
//! THE FIRST WORD decides how the rest is read:
//!
//! ```text
//!   EXPLAIN, CENSUS, LIST, CHECK    that subcommand
//!   a scope word, WHERE, PROPERTY   the grammar above
//!   or PREVIEW
//!   anything else                   the property, as the whole rest of the
//!                                   line: CODE feature_code, as it always was
//! ```
//!
//! A scope word that cannot begin a scope where it stands is not one: LAYER,
//! LAYERS and AREA need the word after them, so CODE Layer - a property many
//! drawings from GIS data carry - is still that property, and CODE LAYERS a is
//! the layer a. A property called as a scope word that CAN stand alone (ALL,
//! VIEW, SEL), or as a subcommand, is given as PROPERTY <name>.
//!
//! So is a property of SEVERAL words whose first is any of those words: with
//! a word after it even LAYER, LAYERS and AREA begin the scope form, and
//! nothing can tell CODE Layer a, the layer, from a property called "Layer a".
//! CODE Area m2 is refused (a window is four numbers) and CODE Layer a codes
//! layer a; that property is PROPERTY "Area m2". A property of several words
//! whose first is none of them is still the whole rest of the line.
//!
//! Every reply begins with what the scope took (scope_record); the rest is
//! code_table's formatter text, the words the Survey Code Manager shows.
//! Colour names are resolved through the Document and nothing else
//! (colour_lookup: the customisation's own table, then the standard names): a
//! name neither knows leaves the colour alone.

use std::sync::Arc;

use katana_commands::CommandPtr;
use katana_core::text::equals_ignoring_case;
use katana_core::{Error, ErrorCode, Result};
use katana_entity::Color;
use katana_entity::colour_names::resolve_colour;
use katana_entity::tables::is_built_in_symbol_name;

use crate::code_table::{
    BuiltInSymbolTest, ColourLookup, DefinitionLookup, LintIssue, LintSeverity, code_census_of,
    code_table, explain_code, format_code_census, format_code_explanation, format_code_table,
    format_coding_report, format_lint, lint_survey_map,
};
use crate::colour_lookup::colour_lookup;
use crate::document::Document;
use crate::scope_verbs::{
    ScopeSource, ScopeViewProvider, ScopeWords, is_scope_word, match_scope, parse_scope_words,
    scope_record,
};
use crate::survey_coding::{
    ColourOf, SurveyCodingOptions, SurveyCodingReport, apply_survey_codes, find_code_property,
};

const APPLY_USAGE: &str = "CODE [<scope>] [WHERE key=value ...] [PROPERTY <name>] [PREVIEW]";
const CENSUS_USAGE: &str =
    "CODE CENSUS [<scope>] [WHERE key=value ...] [PROPERTY <name>] [PREVIEW]";

fn usage(text: &str) -> Error {
    Error::new(ErrorCode::InvalidArgument, format!("usage: {text}"))
}

fn is(word: &str, name: &str) -> bool {
    equals_ignoring_case(word.as_bytes(), name.as_bytes())
}

// The words from `from` on, joined by one blank: a code, a property or a
// filter, which the command line typed as the rest of the line.
fn rest_of(words: &[String], from: usize) -> String {
    words
        .get(from..)
        .map_or_else(String::new, |rest| rest.join(" "))
}

// The formatters end every line with '\n'; a reply does not, since each front
// end ends it (katana_cli prints one, the window's log is a line a message).
fn reply(mut text: String) -> String {
    while text.ends_with('\n') {
        text.pop();
    }
    text
}

// The remedy names the one kind of file that loads: a Katana customisation.
fn require_map(document: &Document) -> katana_core::Status {
    if document.survey_map().empty() {
        return Err(Error::new(
            ErrorCode::InvalidState,
            "no survey codes are loaded; CUSTOMISE <file> loads a Katana customisation file",
        ));
    }
    Ok(())
}

// The Document's colour resolution as the function a survey coding keeps:
// the customisation's table as it is now, then the standard names. A coding
// reads it only while it is planned, during which the Document cannot change,
// so a copy of the table answers as colour_lookup would (a SurveyCodingOptions
// keeps a shared, owned function and cannot borrow the Document).
fn document_colour_of(document: &Document) -> ColourOf {
    let colours = document.customisation_state().colours.clone();
    Arc::new(move |name: &str| -> Option<Color> { resolve_colour(&colours, name) })
}

// What the words after CODE, or after CODE CENSUS, ask for.
struct CodeRequest {
    scope: ScopeWords,
    // Empty: found, as apply_survey_codes finds it.
    property: String,
    preview: bool,
}

// Whether `words[at]` begins the scope form rather than being the property
// the line names by position (the module's comment, "the first word"). A word
// that needs the word after it and has none cannot begin anything: "CODE
// Layer" is the property Layer.
fn begins_scope_form(words: &[String], at: usize) -> bool {
    let word = &words[at];
    let followed = at + 1 < words.len();
    if is(word, "PREVIEW") {
        return true;
    }
    if is(word, "PROPERTY") || is(word, "LAYER") || is(word, "LAYERS") || is(word, "AREA") {
        return followed;
    }
    is_scope_word(word)
}

// [<scope>] [WHERE ...] [PROPERTY <name>] [PREVIEW], the last two in either
// order and on either side of the scope. The scope and its filter are the
// shared parser's; with no scope word it is the DRAWING, a bare WHERE
// included, which is this verb's own default. ONE grammar for CODE and for
// CODE CENSUS: PREVIEW is read for both, and a census, which changes nothing,
// has nothing to do for it.
fn parse_request(words: &[String], start: usize, usage_text: &str) -> Result<CodeRequest> {
    let mut request = CodeRequest {
        scope: ScopeWords {
            source: ScopeSource::Drawing,
            ..ScopeWords::default()
        },
        property: String::new(),
        preview: false,
    };
    if start < words.len() && !begins_scope_form(words, start) {
        // The property by position, as CODE has always taken it: the whole
        // rest of the line, so a name of two words needs no quotes.
        request.property = rest_of(words, start);
        return Ok(request);
    }
    let mut scoped = false;
    let mut named = false;
    let mut at = start;
    while at < words.len() {
        let word = &words[at];
        if is(word, "PROPERTY") {
            if named || at + 1 >= words.len() {
                return Err(usage(usage_text));
            }
            request.property.clone_from(&words[at + 1]);
            named = true;
            at += 2;
            continue;
        }
        if is(word, "PREVIEW") {
            request.preview = true;
            at += 1;
            continue;
        }
        if !scoped && is_scope_word(word) {
            let bare_where = is(word, "WHERE");
            request.scope = parse_scope_words(words, &mut at)?;
            if bare_where {
                // The shared parser reads no scope word as the selection.
                request.scope.source = ScopeSource::Drawing;
            }
            scoped = true;
            continue;
        }
        return Err(usage(usage_text));
    }
    Ok(request)
}

// CODE [<scope>] ... [PREVIEW]: apply the loaded codes to what the scope took.
fn apply(document: &mut Document, args: &[String], views: ScopeViewProvider<'_>) -> Result<String> {
    let request = parse_request(args, 0, APPLY_USAGE)?;
    let found = match_scope(document, &request.scope, views)?;
    let mut text = scope_record(&found);
    text.push('\n');

    let mut report = SurveyCodingReport::default();
    let mut command: Option<CommandPtr> = None;
    if found.matched.is_empty() {
        // A scope that took nothing is reported, not refused - and is NOT
        // handed on: an empty id list means every entity to apply_survey_codes.
        report.property = if request.property.is_empty() {
            find_code_property(document.model(), &[])
        } else {
            request.property.clone()
        };
    } else {
        let options = SurveyCodingOptions {
            property: request.property.clone(),
            ids: found.matched.clone(),
            colour_of: Some(document_colour_of(document)),
            ..SurveyCodingOptions::default()
        };
        command = apply_survey_codes(document, &options, Some(&mut report))?;
    }
    text.push_str(&format_coding_report(&report));
    let Some(command) = command else {
        return Ok(text + "Nothing to change.");
    };
    if request.preview {
        return Ok(text + "Preview: nothing was changed.");
    }
    document.execute(command)?;
    Ok(text + "Applied as one command. UNDO puts it all back.")
}

// CODE CENSUS [<scope>] ...: the codes what the scope took carries. PREVIEW
// is taken and asks nothing more of it: the two forms are one grammar, and a
// line built for CODE with CENSUS put in must not turn into a usage error for
// the one word that promises to change nothing.
fn census(document: &Document, args: &[String], views: ScopeViewProvider<'_>) -> Result<String> {
    let request = parse_request(args, 1, CENSUS_USAGE)?;
    let found = match_scope(document, &request.scope, views)?;
    Ok(format!(
        "{}\n{}",
        scope_record(&found),
        reply(format_code_census(&code_census_of(
            document,
            &found.matched,
            &request.property
        )))
    ))
}

fn code(document: &mut Document, args: &[String], views: ScopeViewProvider<'_>) -> Result<String> {
    let word = args.first().map_or("", String::as_str);
    // CHECK takes no word: one given is a mistake, said before anything else.
    if is(word, "CHECK") && args.len() != 1 {
        return Err(usage("CODE CHECK"));
    }
    if is(word, "EXPLAIN") && args.len() < 2 {
        return Err(usage("CODE EXPLAIN <code>"));
    }
    require_map(document)?;
    if is(word, "EXPLAIN") {
        let shared: &Document = document;
        let definitions = move |name: &str| shared.definition_for(name);
        let definition_of: &DefinitionLookup<'_> = &definitions;
        let colours = colour_lookup(shared);
        let colour_of: &ColourLookup<'_> = &colours;
        let explanation = explain_code(
            shared.survey_map(),
            &rest_of(args, 1),
            Some(definition_of),
            Some(colour_of),
        );
        return Ok(reply(format_code_explanation(&explanation)));
    }
    if is(word, "CENSUS") {
        return census(document, args, views);
    }
    if is(word, "LIST") {
        return Ok(reply(format_code_table(
            &code_table(document.survey_map()),
            &rest_of(args, 1),
        )));
    }
    if is(word, "CHECK") {
        let shared: &Document = document;
        let colours = colour_lookup(shared);
        let colour_of: &ColourLookup<'_> = &colours;
        let built_in: &BuiltInSymbolTest<'_> = &is_built_in_symbol_name;
        let map = shared.survey_map();
        return code_check_reply(
            &lint_survey_map(map, shared.style_library(), Some(colour_of), Some(built_in)),
            map.size(),
        );
    }
    apply(document, args, views)
}

/// True for CODE, in any case.
pub fn is_survey_code_verb(verb: &str) -> bool {
    is(verb, "CODE")
}

/// Runs one CODE line: `args` are the words AFTER the verb, the quotes removed
/// (CommandInterpreter::tokenize). `views` answers the scope word VIEW, and is
/// None headless. InvalidState when no survey codes are loaded; a refused CODE
/// changes nothing.
///
/// A colour name is resolved by the Document alone (colour_lookup): a
/// parameter for a caller's own lookup would be a second way to resolve a
/// colour that no program uses.
pub fn run_survey_code_verb(
    document: &mut Document,
    args: &[String],
    views: ScopeViewProvider<'_>,
) -> Result<String> {
    // The Document's own answer and no other: its customisation's table, then
    // the standard names, asked at each line so that it follows a
    // customisation loaded since.
    code(document, args, views)
}

/// CODE CHECK's answer for the lint of a map of `rules` rules: the lint's
/// text, or - when any issue is an error - an InvalidState refusal whose first
/// line counts the errors and whose later lines are the whole lint, so a
/// script stops there and still shows why (as UTILITY CHECK refuses). Apart
/// from the verb because SurveyMap::add refuses the rules that lint as errors
/// today, so no map a test can build reaches the refusal through the verb.
pub fn code_check_reply(issues: &[LintIssue], rules: usize) -> Result<String> {
    let text = reply(format_lint(issues, rules));
    let errors = issues
        .iter()
        .filter(|issue| issue.severity == LintSeverity::Error)
        .count();
    if errors != 0 {
        return Err(Error::new(
            ErrorCode::InvalidState,
            format!(
                "the loaded survey codes have {errors} {}: a rule cannot be applied as written\n{text}",
                if errors == 1 { "error" } else { "errors" }
            ),
        ));
    }
    Ok(text)
}
