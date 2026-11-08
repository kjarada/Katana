//! LINEWORK: joining coded points into lines from the text command line
//! (docs/survey_coding.md, "Linework"; include/katana/cad/linework_verbs.hpp,
//! src/katana_cad/customisation/linework_verbs.cpp). The interpreter's, so the
//! window's command line, katana_cli, katana_mcp and an AI agent all run the
//! same verb.
//!
//! ```text
//! LINEWORK [<scope>] [WHERE k=v ...] [PROPERTY <name>] [ORDER number|entity]
//!          [CHORD <length>] [PREVIEW]
//! ```
//!
//! <scope> is the shared scope and filter words (scope_verbs.rs), and comes
//! first. With no scope word it is THE SELECTION WHEN ANYTHING IS SELECTED,
//! ELSE THE WHOLE DRAWING - also under a bare WHERE - and the reply says which
//! (scope=). PROPERTY names the property the codes are read from, as CODE's
//! PROPERTY does; without it the property is found (code_property_candidates).
//! ORDER says how the points of one string are ordered: by their point number
//! (the default) or by entity, which for an import is the order the file
//! listed them. CHORD is how far, in the drawing's units, the straight
//! segments drawn for a curve ("BC" ... "EC") may stray from it: LineworkOptions::
//! chord_tolerance, 0.005 unless said. PREVIEW plans and reports, and changes
//! nothing. Each of the verb's own words is given at most once.
//!
//! PROPERTY and CHORD are the words for the two controls of the Survey Code
//! Manager's Linework tab, which runs this line (CLAUDE.md section 1, one code
//! path): a control the line could not say would have had to go. A property is
//! what CODE already takes, so that points coded under a name of the drawing's
//! own can be strung as they were coded; a chord length is geometry no survey
//! code holds.
//!
//! It is process_linework over the points the scope takes, with what the
//! drawing's customisation says of linework: the control codes are the
//! Document's (customisation_state().linework) and the lines are styled with
//! the colours of its resolver (colour_lookup.rs). A line is drawn where a
//! rule makes its code a line or a control code asks for one - the run was
//! asked for by name, so a control code alone still draws, as it does from the
//! Survey Code Manager. process_linework decides that point by point, so where
//! NO rule makes the code a line only the points that themselves carry a
//! control code are joined: of "KB ST", "KB", "KB CL" the first and the third
//! make the line and the second is unplaced, with the reason. It NEVER removes
//! a point. Every line it draws is ONE undo step; a run that draws nothing is
//! no step at all.
//!
//! POINTS THEIR SURVEY JOB HAS ALREADY STRUNG ARE LEFT OUT, and counted. A job
//! imported with linework on (survey_finish.rs) drew its own lines - the
//! file's numbered strings among them, which only the file knows - and owns
//! them: a re-adjustment redraws them in place and Remove Job deletes them. A
//! second line strung through the same points here would lie on top of the
//! job's and be nobody's. How it is known: the job's stored options say it was
//! imported with linework (SurveyJobOptions::draw_linework), the point is among
//! the job's created_entities, and a polyline among those same
//! created_entities, still in the drawing, carries the point's STRING NAME -
//! its code followed by its string number (survey_coding.rs). So a point of
//! such a job whose string the job drew no line of - a rule added since, a
//! line deleted by hand - is still strung here; and a job whose option text
//! cannot be read refuses the run, naming the job, rather than have it guess.
//! Such a job's points are strung BY THEIR CODES, as any points are: a string
//! the file closed and began again under one number is one name, and comes
//! out as one open line.
//!
//! A LINE THE DRAWING ALREADY HOLDS IS NOT DRAWN AGAIN, and is counted
//! (LineworkOptions::skip_lines_already_drawn): a polyline that carries the
//! same code and string number and runs through the same vertices at the same
//! heights, closed alike. The verb is run again and again over the same points -
//! after a rule is written, after more are surveyed - and the job rule above
//! knows nothing of the lines the verb itself drew, nor of an import's that is
//! no job's: without this each run laid the same lines on top of the last. It
//! is the VERY line that is known, not its name: a string that has gained a
//! point, or whose point was moved or re-levelled, is another line and is
//! drawn, and the line that was there stays until someone erases it.
//! (Redrawing it in place, as a job redraws its own, would take whichever line
//! carries that name - and two surveys of one site both have a KB1.)
//!
//! The reply is records, one a line:
//!
//! ```text
//! linework scope=drawing matched=14 considered=9 lines=2 unplaced=3 notes=1
//!                                        (preview=yes after them for a PREVIEW)
//! left_out=4 reason=strung-by-their-job  (points; only when some were left out)
//! left_out=3 reason=already-drawn lines=1
//!                                        (lines= the lines not drawn again, and
//!                                        the points they run through, each once;
//!                                        only when there is one)
//! string name=KB1 key=KB* number=1 points=3 vertices=3 closed=no layer=KERB entity=31
//!                                        (each line, the first 50; then
//!                                        strings_more=<n>; join=yes on a join
//!                                        line; no entity= in a preview)
//! unplaced reason="its code is a point code" points=2
//!                                        (each reason a point is in no line)
//! note kind="unknown token" count=1      (each kind of note)
//! ```
//!
//! matched= is what the scope took (scope_record); considered= the point
//! entities among it that were looked at - the ones left to their job are not
//! among them, the ones of a line already drawn are; lines= the lines drawn
//! (in a preview, to be drawn). A scope that takes nothing, or no point, is
//! answered with the same records and zeros: it is not a failure.
//!
//! THE COLOUR LOOKUP these hand on is the Document's colour table AS IT STANDS
//! when the plan is made, where the C++'s colourLookup asks the Document at
//! each call. The lookup is called while the plan's command runs, inside the
//! Document's own step, where a Rust closure can reach the Document only
//! through its state link - and the styling that calls it already holds that
//! link (StateLink::with does not nest). A plan is executed in the same verb
//! that made it, and a colour table changes only outside a step, so the two
//! answer alike.

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use katana_core::error::make_error;
use katana_core::text::{equals_ignoring_case, parse_finite_double};
use katana_core::{Error, ErrorCode, Result};
use katana_entity::colour_names::resolve_colour;
use katana_entity::{Entity, EntityId, EnumName, Geometry};

use crate::document::Document;
use crate::linework::{
    LineworkCodes, LineworkOptions, LineworkOrder, LineworkResult, parse_field_code,
    process_linework,
};
use crate::scope_verbs::{
    ScopeSource, ScopeViewProvider, ScopeWords, is_scope_word, match_scope, parse_scope_words,
    record_value, scope_record,
};
use crate::survey_coding::{ColourOf, code_property_candidates, survey_code_of, survey_string_of};
use crate::survey_finish::SurveyFinishOptions;
use crate::survey_job::read_survey_job_options;

/// The Document's colours as a lookup a command may keep (the top of this
/// module says why it is the table as it stands).
pub(crate) fn document_colours(document: &Document) -> ColourOf {
    let colours = document.customisation_state().colours.clone();
    Arc::new(move |name: &str| resolve_colour(&colours, name))
}

const USAGE: &str = "LINEWORK [<scope>] [WHERE k=v ...] [PROPERTY <name>] [ORDER number|entity] \
                     [CHORD <length>] [PREVIEW]";

fn is(word: &str, name: &str) -> bool {
    equals_ignoring_case(word.as_bytes(), name.as_bytes())
}

fn not_a_word(word: &str, why: &str) -> Error {
    make_error(ErrorCode::ParseFailure, format!("{why}; {USAGE}"), word)
}

/// How many lines a LINEWORK reply lists, each a record; the rest are counted
/// (strings_more=). A survey of a few thousand points strings a few hundred
/// lines, and a reply is read by a person in a log as well as by an agent.
pub const LINEWORK_STRINGS_LISTED: usize = 50;

// ---- the plan, apart from the words -------------------------------------------------
//
// What a LINEWORK line plans over the entities its scope took: the verb's own
// planning, for a caller that shows the WHOLE plan before the line is run.
// The Survey Code Manager's Linework tab is one - its Preview lists every
// line, every point in none and every note, where the reply counts them - and
// its Execute then runs the line. What it previewed must therefore be planned
// by the function the line is planned by, not by a second reading of the
// rules at the top of this module (the Document's control codes and colours,
// no point removed, the points their job has strung left out, a line already
// drawn not drawn again): two readings come to differ, and the preview would
// then show something Execute does not do.

/// What a line says beyond its scope and PREVIEW.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LineworkWords {
    /// PROPERTY. Empty: found, as apply_survey_codes finds it.
    pub property: String,
    /// ORDER.
    pub order: LineworkOrder,
    /// CHORD, in model units. None: LineworkOptions::chord_tolerance as it
    /// stands.
    pub chord: Option<f64>,
}

#[derive(Debug, Default)]
pub struct LineworkPlan {
    /// Of the entities asked about, the points their survey job has already
    /// strung: left out of `planned`. In the order asked.
    pub strung_by_their_job: Vec<EntityId>,
    /// process_linework over the rest. An empty report and no command when
    /// nothing is left to look at: an empty list is never handed on, because
    /// to process_linework it means every point in the drawing.
    pub planned: LineworkResult,
}

// What the line asks for, read and not yet looked up.
#[derive(Default)]
struct LineworkLine {
    scope: ScopeWords,
    // A scope word was given; without one the scope is the selection when
    // there is one, else the drawing.
    scope_named: bool,
    words: LineworkWords,
    preview: bool,
}

fn parse(args: &[String]) -> Result<LineworkLine> {
    let mut line = LineworkLine::default();
    let mut at = 0_usize;
    // WHERE alone names no scope: it filters whatever the default is.
    line.scope_named = args
        .first()
        .is_some_and(|first| is_scope_word(first) && !is(first, "WHERE"));
    line.scope = parse_scope_words(args, &mut at)?;

    let mut ordered = false;
    let mut named = false;
    while at < args.len() {
        let word = &args[at];
        if is(word, "PREVIEW") {
            line.preview = true;
            at += 1;
            continue;
        }
        if is(word, "ORDER") {
            if ordered {
                return Err(not_a_word(word, "ORDER is given twice"));
            }
            if at + 1 >= args.len() {
                return Err(not_a_word(word, "ORDER needs number or entity after it"));
            }
            at += 1;
            let how = &args[at];
            if is(how, "number") {
                line.words.order = LineworkOrder::PointNumber;
            } else if is(how, "entity") {
                line.words.order = LineworkOrder::EntityOrder;
            } else {
                return Err(not_a_word(
                    how,
                    "ORDER takes number (by point number) or entity (the order the points were \
                     drawn in)",
                ));
            }
            ordered = true;
            at += 1;
            continue;
        }
        if is(word, "PROPERTY") {
            if named {
                return Err(not_a_word(word, "PROPERTY is given twice"));
            }
            // An empty name ("") is no name: left to mean "find it", it would
            // read a line that names a property as one that names none.
            if at + 1 >= args.len() || args[at + 1].is_empty() {
                return Err(not_a_word(
                    word,
                    "PROPERTY needs the property's name after it",
                ));
            }
            at += 1;
            line.words.property.clone_from(&args[at]);
            named = true;
            at += 1;
            continue;
        }
        if is(word, "CHORD") {
            if line.words.chord.is_some() {
                return Err(not_a_word(word, "CHORD is given twice"));
            }
            if at + 1 >= args.len() {
                return Err(not_a_word(word, "CHORD needs a length after it"));
            }
            at += 1;
            let length = &args[at];
            // Refused here, by name, and not left to process_linework: its
            // refusal names an option of a struct, not a word of this line.
            match parse_finite_double(length.as_bytes()) {
                Some(value) if value > 0.0 => line.words.chord = Some(value),
                _ => {
                    return Err(not_a_word(
                        length,
                        "CHORD takes a length greater than 0, in the drawing's units",
                    ));
                }
            }
            at += 1;
            continue;
        }
        // A scope after the verb's own words would be read by nobody: the
        // shared parser has finished, and guessing that it was meant is how a
        // line strings the whole drawing where it was to string one layer.
        if is_scope_word(word) {
            return Err(not_a_word(word, "the scope and its WHERE come first"));
        }
        return Err(not_a_word(word, "not a LINEWORK word"));
    }
    Ok(line)
}

// The string name `entity` carries under `property`: the first token of its
// code - how a point's code is read everywhere, its control codes left out -
// followed by its string number. Empty with no code. A line drawn by linework
// carries its points' code and string number, so a line and the points it was
// strung through have one name.
fn string_name_of(entity: &Entity, property: &str) -> String {
    let Some(code) = survey_code_of(entity, property) else {
        return String::new();
    };
    let mut name = parse_field_code(code, &LineworkCodes::default()).name;
    if !name.is_empty() {
        name.push_str(survey_string_of(entity));
    }
    name
}

// The points of `subject` that the survey job which drew them has strung
// already (the top of this module has the rule and the reason).
fn strung_by_their_job(document: &Document, subject: &[EntityId]) -> Result<HashSet<EntityId>> {
    let mut strung = HashSet::new();
    if subject.is_empty() || document.survey_jobs().is_empty() {
        return Ok(strung);
    }
    let asked: HashSet<EntityId> = subject.iter().copied().collect();
    let entities = &document.model().entities;
    for job in document.survey_jobs() {
        // A job that owns nothing the scope took is not read at all: its
        // options, damaged or written by a newer Katana, are then no reason to
        // refuse a run that does not touch it.
        if !job.created_entities.iter().any(|id| asked.contains(id)) {
            continue;
        }
        let stored = match read_survey_job_options(&job.import_options, &job.id) {
            Ok(stored) => stored,
            Err(error) => {
                let context = if error.context.is_empty() {
                    format!(
                        "LINEWORK cannot tell whether survey job {} strung its own points",
                        job.id
                    )
                } else {
                    format!(
                        "LINEWORK cannot tell whether survey job {} strung its own points; {}",
                        job.id, error.context
                    )
                };
                return Err(make_error(error.code, error.message, context));
            }
        };
        if !stored.draw_linework {
            continue;
        }
        let property = if stored.import.code_property.is_empty() {
            code_property_candidates()[0]
        } else {
            stored.import.code_property.as_str()
        };
        let mut drawn: HashSet<String> = HashSet::new();
        for &id in &job.created_entities {
            if let Some(line) = entities.find(id)
                && matches!(line.geometry, Geometry::Polyline(_))
            {
                let name = string_name_of(line, property);
                if !name.is_empty() {
                    drawn.insert(name);
                }
            }
        }
        if drawn.is_empty() {
            continue;
        }
        for &id in &job.created_entities {
            if !asked.contains(&id) {
                continue;
            }
            if let Some(point) = entities.find(id)
                && matches!(point.geometry, Geometry::Point(_))
                && drawn.contains(&string_name_of(point, property))
            {
                strung.insert(id);
            }
        }
    }
    Ok(strung)
}

/// True for LINEWORK, in any case.
pub fn is_linework_verb(verb: &str) -> bool {
    is(verb, "LINEWORK")
}

/// Plans, and changes nothing. `matched` is what a scope took (match_scope, or
/// match_entities for a caller that holds a ModifyScope and ModifyFilter). The
/// errors are run_linework_verb's, less the grammar's and the scope's: a
/// survey job whose option text cannot be read, and process_linework's own (a
/// chord length that is not a positive finite number among them). The
/// command, when there is one, must be executed on `document`.
pub fn plan_linework(
    document: &Document,
    matched: &[EntityId],
    words: &LineworkWords,
) -> Result<LineworkPlan> {
    let strung = strung_by_their_job(document, matched)?;
    let mut options = LineworkOptions {
        property: words.property.clone(),
        order: words.order,
        keep_points: true, // the verb never removes a point
        // The verb is run again and again over the same points - after a rule
        // is added, after more are surveyed - and a line it finds drawn is not
        // drawn on top of itself (the top of this module, "A LINE THE DRAWING
        // ALREADY HOLDS").
        skip_lines_already_drawn: true,
        codes: document.customisation_state().linework.clone(),
        ..LineworkOptions::default()
    };
    if let Some(chord) = words.chord {
        options.chord_tolerance = chord;
    }
    options.coding.colour_of = Some(document_colours(document));
    let mut plan = LineworkPlan::default();
    for &id in matched {
        if strung.contains(&id) {
            plan.strung_by_their_job.push(id);
        } else {
            options.ids.push(id);
        }
    }

    // Never with an empty list: to process_linework that is every point in
    // the drawing, and here it is a scope that took nothing, or only points
    // their job has strung.
    if !options.ids.is_empty() {
        plan.planned = process_linework(document, &options)?;
    }
    Ok(plan)
}

// "unplaced reason=..." and "note kind=...": how many of each, keyed by the
// reason's place in its enumeration so they are said in the order declared,
// and two runs over the same drawing answer alike.
type Tally = BTreeMap<usize, (&'static str, usize)>;

fn tally(counts: &mut Tally, place: usize, name: &'static str) {
    counts.entry(place).or_insert((name, 0)).1 += 1;
}

/// Runs one LINEWORK line: `tokens` is the whole line split into words with
/// the quotes removed (CommandInterpreter::tokenize), the verb first. `views`
/// answers the scope word VIEW (the window's; None headless, the C++
/// default).
///
/// ParseFailure for a word the grammar at the top does not take, naming it;
/// the scope's own refusals (scope_verbs.rs); a survey job's ParseFailure or
/// Unsupported when its option text cannot be read and it owns a point the
/// scope took; process_linework's own error otherwise (control codes spelled
/// alike, a rule's layer that is not a layer name). A refused run changes
/// nothing.
pub fn run_linework_verb(
    document: &mut Document,
    tokens: &[String],
    views: ScopeViewProvider<'_>,
) -> Result<String> {
    if tokens.first().is_none_or(|verb| !is_linework_verb(verb)) {
        return Err(make_error(
            ErrorCode::ParseFailure,
            "not a LINEWORK command; type HELP LINEWORK",
            tokens.first().cloned().unwrap_or_default(),
        ));
    }
    let mut line = parse(&tokens[1..])?;
    if !line.scope_named {
        line.scope.source = if document.selection().empty() {
            ScopeSource::Drawing
        } else {
            ScopeSource::Selection
        };
    }
    let found = match_scope(document, &line.scope, views)?;

    let mut plan = plan_linework(document, &found.matched, &line.words)?;
    let mut created: Vec<EntityId> = Vec::new();
    if !line.preview
        && let Some(command) = plan.planned.command.take()
    {
        document.execute(command)?;
        created = document.last_created_entities();
    }
    let strung = &plan.strung_by_their_job;
    let report = &plan.planned.report;

    let mut reply = format!(
        "linework {} considered={} lines={} unplaced={} notes={}",
        scope_record(&found),
        report.considered,
        report.strings.len(),
        report.unplaced.len(),
        report.notes.len()
    );
    if line.preview {
        reply.push_str(" preview=yes");
    }
    if !strung.is_empty() {
        reply.push_str(&format!(
            "\nleft_out={} reason=strung-by-their-job",
            strung.len()
        ));
    }
    if !report.already_drawn.is_empty() {
        // Points, as the record above counts: those the lines not drawn again
        // run through, each once - a join and a run may share one.
        let mut through: HashSet<EntityId> = HashSet::new();
        for drawn in &report.already_drawn {
            through.extend(drawn.points.iter().copied());
        }
        reply.push_str(&format!(
            "\nleft_out={} reason=already-drawn lines={}",
            through.len(),
            report.already_drawn.len()
        ));
    }
    // The lines are created in the order the report lists them, so the n-th
    // created entity is the n-th string; said only where the two lists are
    // the same length, which they are whenever every line is a new entity.
    let with_ids = created.len() == report.strings.len();
    for (i, string) in report
        .strings
        .iter()
        .take(LINEWORK_STRINGS_LISTED)
        .enumerate()
    {
        reply.push_str(&format!(
            "\nstring name={} key={} number={} points={} vertices={} closed={} layer={}",
            record_value(&string.name),
            record_value(&string.key),
            record_value(&string.number),
            string.point_numbers.len(),
            string.vertices,
            if string.closed { "yes" } else { "no" },
            record_value(&string.layer)
        ));
        if string.join {
            reply.push_str(" join=yes");
        }
        if with_ids {
            reply.push_str(&format!(" entity={}", created[i]));
        }
    }
    if report.strings.len() > LINEWORK_STRINGS_LISTED {
        reply.push_str(&format!(
            "\nstrings_more={}",
            report.strings.len() - LINEWORK_STRINGS_LISTED
        ));
    }
    let mut reasons = Tally::new();
    for point in &report.unplaced {
        tally(&mut reasons, point.reason as usize, point.reason.name());
    }
    for (reason, points) in reasons.values() {
        reply.push_str(&format!(
            "\nunplaced reason={} points={points}",
            record_value(reason)
        ));
    }
    let mut kinds = Tally::new();
    for note in &report.notes {
        tally(&mut kinds, note.kind as usize, note.kind.name());
    }
    for (kind, notes) in kinds.values() {
        reply.push_str(&format!("\nnote kind={} count={notes}", record_value(kind)));
    }
    Ok(reply)
}

/// The verb with every word it takes and what the reply holds, for HELP
/// LINEWORK.
pub fn linework_verb_help() -> String {
    String::from(
        r#"LINEWORK [<scope>] [WHERE k=v ...] [PROPERTY <name>] [ORDER number|entity]
         [CHORD <length>] [PREVIEW]

Joins coded survey points into lines (docs/survey_coding.md, "Linework"): the points of
one string name - a point's code, its control codes left out, followed by its string
number - in order, where a survey code rule makes that code a line. The lines go on
their rule's layer and wear its style, with the customisation's colours. No point is
removed. The lines drawn are ONE undo step.

  <scope>   SELECTION | DRAWING | VIEW [id] [EXTENTS] | AREA x0,y0,x1,y1 | LAYERS a,b
            [ONLY], then [WHERE key=value ...] (HELP: "Scope"). It comes first. With no
            scope word: the selection when anything is selected, else the whole drawing.
  PROPERTY  the property the codes are read from, as CODE's PROPERTY: for points whose
            code is kept under a name of the drawing's own. Without it the property is
            found (code, then the other names a code is usually kept under).
  ORDER     number: by point number, 9 before 10 (the default; a point with none is not
            placed).  entity: the order the points were drawn in, which for an import is
            the order the file listed them.
  CHORD     how far, in the drawing's units, the straight segments drawn for a curve
            (BC ... EC) may stray from it: 0.005 unless said. Greater than 0.
  PREVIEW   plans and reports; nothing is drawn.

Control codes after the code: ST start, END end, CL close, BC and EC a curve, JPN <n> a
join to point n, RECT a rectangle - or as the drawing's customisation spells them.
A code that NO rule makes a line is strung only through the points that carry a
control code themselves: of "KB ST", "KB", "KB CL" with no rule for KB, the first and
the third are joined and the second is in no line, which the reply says.

Two things are left out, and counted:
  - points their survey job has already strung: a job imported with linework on drew
    and owns its lines, and a second line through the same points would lie on top of
    them;
  - a line the drawing already holds - the same code, string number, vertices and
    heights - so LINEWORK run again draws nothing twice. A string that has gained a
    point, or whose point was moved, is another line: it is drawn, and the line that
    was there stays until it is erased.
SURVEY IMPORT ... LINEWORK off imports a job that strings nothing, and LINEWORK then
strings its points BY THEIR CODES, not as the file numbered and closed its strings: a
string the file closed and began again under one number comes out as one open line.

The reply is key=value records:
  linework scope=... matched=<n> considered=<n> lines=<n> unplaced=<n> notes=<n>
            matched: what the scope took; considered: the points among it looked at;
            lines: those drawn. preview=yes after them for a PREVIEW
  left_out=<n> reason=strung-by-their-job      n points left to their job's lines
  left_out=<n> reason=already-drawn lines=<m>  m lines the drawing already holds, and
            the n points they run through (each record only when it has something)
  string name=<n> key=<k> number=<n> points=<n> vertices=<n> closed=yes|no layer=<l>
            [join=yes] entity=<id>      each line, the first 50, then strings_more=<n>
  unplaced reason="<why>" points=<n>           each reason a point is in no line
  note kind="<what>" count=<n>                 each kind of note
"#,
    )
}

// ---- what a survey import does beyond drawing its points ---------------------------
//
// The finish of survey_finish.rs as the drawing's customisation asks for it.
// ONE place decides it, so SURVEY IMPORT and the import wizard cannot come to
// differ: the test that holds the two together compares the whole job.
//
//   WHETHER  What the line or the dialog said (`codes`, `linework`); where it
//            said nothing, the Document's automation switch
//            (customisation_state().automation - both on until a
//            customisation or a setting says otherwise).
//   NOT AT ALL WITH NO SURVEY CODES LOADED. `options.codes` and
//            `options.linework` are then off whatever was asked, and the
//            import is the plain one to the last byte - the same job text,
//            the same report. The finish would draw nothing either way
//            (SurveyFinishSkip::NoSurveyCodes), but asked for it would still
//            store the job as one to finish and add two warnings to its
//            report; and a re-adjustment codes only the points it draws for
//            the first time, so a job stored that way would come out half
//            coded once codes were loaded. A job imported with no codes
//            loaded is a job that was not coded, and is kept as one.
//   HOW      Colours through the Document's resolver (colour_lookup.rs, as
//            the top of this module takes it), the control codes the
//            Document's (customisation_state().linework), strings ordered by
//            point number.

#[derive(Clone, Debug, Default)]
pub struct SurveyImportFinish {
    /// Said so, or the Document's switch is on.
    pub codes_asked: bool,
    /// Likewise.
    pub linework_asked: bool,
    /// The drawing has survey codes; without them nothing is handed on.
    pub survey_codes_loaded: bool,
    /// What to hand the import (SurveyJobImport::finish, with_survey_finish).
    pub options: SurveyFinishOptions,
}

/// The C++ defaults of `codes` and `linework` are None: what the Document's
/// automation says.
pub fn survey_import_finish(
    document: &Document,
    codes: Option<bool>,
    linework: Option<bool>,
) -> SurveyImportFinish {
    let automation = document.customisation_state().automation;
    let codes_asked = codes.unwrap_or(automation.codes_on_survey_import);
    let linework_asked = linework.unwrap_or(automation.linework_on_survey_import);
    let survey_codes_loaded = !document.survey_map().empty();
    let mut options = SurveyFinishOptions {
        codes: codes_asked && survey_codes_loaded,
        linework: linework_asked && survey_codes_loaded,
        controls: document.customisation_state().linework.clone(),
        order: LineworkOrder::PointNumber,
        ..SurveyFinishOptions::default()
    };
    options.coding.colour_of = Some(document_colours(document));
    SurveyImportFinish {
        codes_asked,
        linework_asked,
        survey_codes_loaded,
        options,
    }
}
