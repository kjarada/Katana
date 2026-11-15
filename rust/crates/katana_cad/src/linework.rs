//! Field-to-finish linework: joining coded survey points into lines
//! (include/katana/cad/linework.hpp, src/katana_cad/customisation/linework.cpp).
//!
//! A surveyor walking a kerb shoots "KB1", "KB1", "KB1" and expects a line, not
//! three dots. The mapfile says WHICH codes are lines (SurveyRule::breakline)
//! and how they are drawn; this is what actually joins them. Civil 3D ("Process
//! Linework"), TBC ("Process Feature Codes"), Carlson ("Field to Finish") and
//! other survey packages' field reduction all do it, and until this Katana had
//! nothing.
//!
//! Three decisions the rest of this module builds on:
//!
//! ```text
//! 1. WHICH POINTS FORM ONE STRING. The first token of a point's code is its
//!    STRING NAME: every point named "WM01" is one water main, every point
//!    named "WM02" another. split_string_name says how that name divides into
//!    the rule key it is coded by ("WM*") and a string number ("01"). The
//!    split is reported rather than used for grouping: two names are two
//!    strings whatever they split into, because "WM01" and "WM1" are two
//!    strings in the field. A point whose file kept the number apart from the
//!    code carries it beside the code (SURVEY_STRING_PROPERTY,
//!    survey_coding.rs), and its string name is the two together: "KJ" in
//!    string "01" and "KJ" in string "02" are two strings, and a point written
//!    "KJ01" whole is in the first of them.
//!
//! 2. CONTROL CODES ARE DATA. A survey code file has no start/end/close codes
//!    - it forms strings by name alone - so what "ST" means is not in any
//!    file Katana reads. LineworkCodes holds the spellings, with defaults
//!    that are common field conventions and nothing more. They are
//!    configurable, and they are NOT survey code file data.
//!
//! 3. NOTHING IS DROPPED SILENTLY. Every point that is not in a line is in the
//!    report with the reason, and every token that was not understood is in
//!    the report with the point it was on. A point left out of a kerb line
//!    looks exactly like a kerb that was surveyed short.
//! ```
//!
//! What the model can carry, and so what this builds:
//!
//! ```text
//! * Polyline2 is straight segments only - it has no bulge or arc segment. A
//!   curve ("BC" ... "EC") is therefore CHORDED: densified into straight
//!   segments whose sagitta (the gap between chord and arc) is at most
//!   LineworkOptions::chord_tolerance. The report counts the curves so
//!   chorded.
//! * A polyline has no Z of its own. Heights are kept per vertex in the
//!   `elevations` property through entity's set_heights, the one writer the
//!   archive import and the survey import share; a point with no height gives
//!   its vertex no height (absent is not zero).
//! ```
//!
//! The lines are styled by CALLING apply_survey_codes on them, when the
//! command runs, rather than by a second copy of its naming here: a line coded
//! "WM01" then wears exactly the style a point coded "WM01" would, and a change
//! to how that style is chosen reaches both at once.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use katana_commands::{
    ChangeSet, ChangeSetCommand, Command, CommandContext, CommandPtr, Transaction, create_entities,
    create_layer, delete_entities,
};
use katana_core::error::make_error;
use katana_core::text::{equals_ignoring_case, format_exact_real, parse_finite_double};
use katana_core::{Error, ErrorCode, Result, Status};
use katana_entity::entity::{INVALID_ENTITY_ID, heights_of, property_to_string, set_heights};
use katana_entity::layer_path::validate_layer_path;
use katana_entity::{
    Entity, EntityId, EnumName, Geometry, Layer, Model, PropertyValue, SurveyBreakline, SurveyMap,
    SurveyMatch,
};
use katana_geometry::{Arc2, Point2, Polyline2, Vec2};
use katana_math::numerics::tolerance;
use katana_math::{cpp, lerp, normalize_angle};
use katana_survey::data_model::{SurveyPoint, SurveyProject, validate_project};

use crate::document::{Document, StateLink};
use crate::survey_coding::{
    CodingSource, SURVEY_STRING_PROPERTY, SurveyCodingOptions, SurveyCodingReport,
    apply_survey_codes_to, code_property_candidates, survey_lookup_name, survey_string_of,
};
use crate::survey_import::{SurveyImportOptions, layer_for_point};

// CONFIGURABLE, NOT SURVEY CODE FILE DATA. The spellings are part of a
// customisation, so the type and its validate live in the entity layer, where
// a customisation is read and written (entity's linework_codes.rs: the seven
// controls, their defaults and where those come from). These two names keep
// `linework::LineworkCodes` and `linework::validate(codes)` meaning what they
// did: the entity's type and its validate itself, re-exported, not copies.
pub use katana_entity::LineworkCodes;
pub use katana_entity::validate;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

// ---- string names -------------------------------------------------------------

/// How a string name divides into the rule that codes it and a string number.
///
/// THE RULE: the key of the most specific rule that matches the name decides
/// it. A prefix key "WM*" takes its prefix ("WM") and leaves the remainder as
/// the number, so "WM01" is key "WM*", number "01". An exact key ("PABB") is
/// the whole name, so there is no number: one string per code. "WM" under
/// "WM*" likewise has no number. When the most specific rule is the bare "*"
/// the name is FALLBACK-ONLY (audit CAD-05's word): nothing specific knows it,
/// so it is its own key with no number rather than "*" plus the whole name as
/// a "number". No rule at all gives an empty key and `matched` false.
///
/// "Most specific" is SurveyMap::match's order - exact, then the longest
/// prefix - so this can never disagree with the rule that styles the line.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StringName {
    /// "WM*", "PABB", "*" or empty.
    pub key: String,
    /// "01", or empty.
    pub number: String,
    /// Some rule matched, "*" included.
    pub matched: bool,
    /// ... and it was only "*".
    pub fallback_only: bool,
}

pub fn split_string_name(map: &SurveyMap, name: &str) -> StringName {
    let mut split = StringName::default();
    let rules = map.r#match(name);
    let Some(best) = rules.first() else {
        return split;
    };
    split.matched = true;
    let key = &best.key; // the most specific
    if key == "*" {
        split.key = String::from(name);
        split.fallback_only = true;
        return split;
    }
    split.key.clone_from(key);
    if key.ends_with('*') {
        split.number = String::from(name.get(key.len() - 1..).unwrap_or_default());
    }
    split
}

// ---- control codes --------------------------------------------------------------

/// The words the seven controls are known by - "start", "arcStart" - are
/// entity's linework_code_members(), the one list of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LineworkControl {
    Start,
    End,
    Close,
    ArcStart,
    ArcEnd,
    Join,
    Rectangle,
}

/// A point's code taken apart.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FieldCode {
    /// The first token: the string name, "KB1".
    pub name: String,
    /// In the order written.
    pub controls: Vec<LineworkControl>,
    /// The point number after the join token; empty when there was none (which
    /// is itself reported as a note, not ignored).
    pub join_to: String,
    pub join_without_target: bool,
    /// Every token after the name that is neither a control nor the join's
    /// point number, as written. Reported, never silently ignored.
    pub unknown_tokens: Vec<String>,
}

impl FieldCode {
    pub fn has(&self, control: LineworkControl) -> bool {
        self.controls.contains(&control)
    }
}

/// What core's is_ascii_space calls a blank.
const ASCII_SPACE: [char; 6] = [' ', '\t', '\n', '\x0b', '\x0c', '\r'];

fn spellings(codes: &LineworkCodes) -> [(LineworkControl, &str); 7] {
    [
        (LineworkControl::Start, codes.start.as_str()),
        (LineworkControl::End, codes.end.as_str()),
        (LineworkControl::Close, codes.close.as_str()),
        (LineworkControl::ArcStart, codes.arc_start.as_str()),
        (LineworkControl::ArcEnd, codes.arc_end.as_str()),
        (LineworkControl::Join, codes.join.as_str()),
        (LineworkControl::Rectangle, codes.rectangle.as_str()),
    ]
}

fn spells(spelling: &str, token: &str) -> bool {
    !spelling.is_empty() && equals_ignoring_case(token.as_bytes(), spelling.as_bytes())
}

/// Splits `code` on ASCII blanks. The FIRST token is always the string name,
/// even when it is spelled like a control: a code "CL" is a string named CL.
/// Controls after it are matched IGNORING ASCII CASE: they are keywords typed
/// on a controller keypad, and "st" there means what "ST" means. The string
/// name keeps its case, because the mapfile's keys are matched with case.
pub fn parse_field_code(code: &str, codes: &LineworkCodes) -> FieldCode {
    let tokens: Vec<&str> = code
        .split(ASCII_SPACE)
        .filter(|token| !token.is_empty())
        .collect();

    let mut parsed = FieldCode::default();
    let Some(first) = tokens.first() else {
        return parsed;
    };
    parsed.name = String::from(*first);
    let all = spellings(codes);
    let mut i = 1;
    while i < tokens.len() {
        let token = tokens[i];
        let Some(&(control, _)) = all.iter().find(|(_, spelling)| spells(spelling, token)) else {
            parsed.unknown_tokens.push(String::from(token));
            i += 1;
            continue;
        };
        parsed.controls.push(control);
        if control == LineworkControl::Join {
            // The next token is the point to join to, unless it is itself a
            // control - "JPN CL" is a join with its number forgotten, not a
            // join to a point called CL.
            let next = i + 1 < tokens.len()
                && !all
                    .iter()
                    .any(|(_, spelling)| spells(spelling, tokens[i + 1]));
            if next {
                i += 1;
                parsed.join_to = String::from(tokens[i]);
            } else {
                parsed.join_without_target = true;
            }
        }
        i += 1;
    }
    parsed
}

// ---- processing ------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LineworkOrder {
    /// By the point number property, numerically where the number is a number
    /// ("9" before "10"); numbers before names that are not numbers, names in
    /// byte order; ties in entity order. A point without a number is NOT placed
    /// (it has no place in that order), and is reported.
    #[default]
    PointNumber,
    /// By entity id: creation order, which for an imported survey is the order
    /// the file listed the points - the order they were observed.
    EntityOrder,
}

#[derive(Clone, Debug)]
pub struct LineworkOptions {
    /// The property holding the code. EMPTY means find it, exactly as
    /// apply_survey_codes does: the first of code_property_candidates() that
    /// any point carries.
    pub property: String,
    /// Where the point number is, as survey import writes it.
    pub point_number_property: String,
    pub order: LineworkOrder,
    /// Only these entities, or every point in the drawing when empty. Entities
    /// among them that are not points are counted, not processed.
    pub ids: Vec<EntityId>,
    /// Off: the points a run of their own string went into are deleted in the
    /// same command - the line now stands for them. Points not in any line
    /// are always kept, and so is a point only a join ("JPN") reached: a tree
    /// or an uncoded control point joined to is not replaced by the join line.
    pub keep_points: bool,
    pub codes: LineworkCodes,
    /// The largest gap, in model units, between a chord and the curve it stands
    /// for. 5 mm: below the accuracy of a detail survey, so the chords are not
    /// the error a person sees, and a 10 m radius needs only ~25 chords per
    /// quarter turn.
    pub chord_tolerance: f64,
    /// How the lines are styled: passed to apply_survey_codes with `property`
    /// and `ids` replaced by the lines' own. Pass `colour_of` here as for
    /// apply_survey_codes. `create_layers` also governs the layers the lines
    /// are put on.
    pub coding: SurveyCodingOptions,
    /// On, a string is a line only when a rule MORE SPECIFIC than the bare "*"
    /// makes its name one: neither the fallback rule's breakline nor a control
    /// code alone draws anything, and such a point is unplaced as NoRule. For
    /// a run nobody asked for by name - the one an import makes of its own
    /// points (survey_finish) - where a line no rule of the customisation
    /// describes would be a surprise, not a result. Off, as it always was: the
    /// resolved breakline or a control code makes a line, and the report notes
    /// which lines had no rule.
    pub only_ruled_lines: bool,
    /// Lines an EARLIER run drew of these same points - a survey job's, when
    /// it is re-adjusted (survey_finish). A line planned now that carries the
    /// code and the string number one of them carries IS that line again: the
    /// polyline keeps its entity - its id, its layer, its style, its other
    /// properties, and so every label, leader and dimension that follows it -
    /// and is given the vertices and heights planned now, where a new line
    /// would be created, layered and styled. They are taken in order: the
    /// first such line planned takes the earliest of them (the lowest id).
    /// Only a polyline is taken; the ones no line planned here takes are left
    /// exactly as they are, and LineworkString::redrawn says which were taken.
    /// Empty, as it always was: every line is a new entity.
    pub earlier_lines: Vec<EntityId>,
    /// On, a line planned now that THE DRAWING ALREADY HOLDS is not drawn a
    /// second time: a polyline that carries the same code and string number
    /// and runs through the same vertices at the same heights, closed alike -
    /// whatever drew it, and whatever layer and style it has been given
    /// since. It is listed in LineworkReport::already_drawn in place of
    /// `strings`, its points are in a line (not unplaced) and none of them is
    /// removed on its account, whatever keep_points says: the line this run
    /// would stand in for them with is not this run's. For a run a person
    /// repeats over the same points (the LINEWORK verb), where a second line
    /// on top of the first is nobody's and looks like nothing. Only the very
    /// line: a string that has gained a point, or whose point was moved or
    /// re-levelled, is another line and is drawn. A line taken as one of
    /// `earlier_lines` is redrawn as that says and is not asked about here.
    /// Off, as it always was - and as an import's own run must have it: two
    /// imports of one file each draw, and own, their lines.
    pub skip_lines_already_drawn: bool,
}

impl Default for LineworkOptions {
    fn default() -> Self {
        Self {
            property: String::new(),
            point_number_property: String::from("point"),
            order: LineworkOrder::PointNumber,
            ids: Vec::new(),
            keep_points: true,
            codes: LineworkCodes::default(),
            chord_tolerance: 0.005,
            coding: SurveyCodingOptions::default(),
            only_ruled_lines: false,
            earlier_lines: Vec::new(),
            skip_lines_already_drawn: false,
        }
    }
}

/// Why a point is not in any line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum UnplacedReason {
    /// No code in the property.
    #[default]
    NoCode,
    /// No rule for its name and no control code on it.
    NoRule,
    /// Its rules make it a point (breakline Point) or never say it is a line.
    PointCode,
    /// Ordering by number, and it has none.
    NoPointNumber,
    /// The only point of its string: one point makes no line.
    LonePoint,
    /// Every point of its string is in one place: no length to draw.
    Coincident,
}

/// The C++ `toString(UnplacedReason)`.
impl EnumName for UnplacedReason {
    fn name(self) -> &'static str {
        match self {
            Self::NoCode => "no code",
            Self::NoRule => "no rule for its code and no control code",
            Self::PointCode => "its code is a point code",
            Self::NoPointNumber => "no point number to order it by",
            Self::LonePoint => "the only point of its string",
            Self::Coincident => "every point of its string is in one place",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UnplacedPoint {
    pub id: EntityId,
    /// Empty when it has none.
    pub point_number: String,
    /// As the property holds it.
    pub code: String,
    pub reason: UnplacedReason,
}

/// Things worth telling that did not stop a point being placed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LineworkNoteKind {
    /// A token that is neither a control nor a join target.
    #[default]
    UnknownToken,
    /// "JPN" with no point number after it.
    JoinWithoutTarget,
    /// No point has that number (or several do).
    JoinTargetMissing,
    /// "BC" then "EC" with no point between: drawn straight.
    CurveTooShort,
    /// Three curve points in a straight line: drawn straight, not counted.
    CurveCollinear,
    /// "BC" with no "EC": taken to end at the string's last point, or - begun
    /// on that point - nothing curved. The detail says which.
    CurveUnterminated,
    /// "EC" with no open curve: nothing to end.
    CurveEndWithoutStart,
    /// "CL" on a string of two points: drawn open.
    CloseTooShort,
    /// "RECT" on other than three points with width, so no rectangle is
    /// constructed and the points are drawn as they are: closed through them
    /// when there are more than three, open when there are two or when the
    /// third lies on the first side's line. The detail says which.
    RectangleShape,
    /// Two points of one string share a number.
    DuplicatePointNumber,
    /// A line by control code alone: nothing styles it.
    NoRuleForName,
    /// A line only because the bare "*" rule says so.
    FallbackOnlyName,
    /// draw_survey_features only: a feature names a point the file gives no
    /// coordinates for (the survey's UnpositionedPoint). Left out of the line -
    /// it has nowhere to be - and said so, because the line then runs straight
    /// past a point the surveyor strung into it.
    UnpositionedPoint,
}

/// The C++ `toString(LineworkNoteKind)`.
impl EnumName for LineworkNoteKind {
    fn name(self) -> &'static str {
        match self {
            Self::UnknownToken => "unknown token",
            Self::JoinWithoutTarget => "join with no point number",
            Self::JoinTargetMissing => "join target not found",
            Self::CurveTooShort => "curve with no point between its ends, drawn straight",
            Self::CurveCollinear => "curve points in a straight line, drawn straight",
            // The detail says what was drawn instead.
            Self::CurveUnterminated => "curve never ended",
            Self::CurveEndWithoutStart => "curve end with no curve begun",
            Self::CloseTooShort => "closing a string of two points, drawn open",
            // Whether the points were then drawn open or closed depends on how
            // many there were, so the detail says which.
            Self::RectangleShape => "a rectangle needs three points with width: none constructed",
            Self::DuplicatePointNumber => "two points of one string share a number",
            // process_linework meets this only for a line made by a control
            // code; draw_survey_features for a feature whose code no rule knows.
            Self::NoRuleForName => "no rule for its code: nothing styles the line",
            Self::FallbackOnlyName => "a line only because the fallback rule \"*\" says so",
            Self::UnpositionedPoint => "a point with no coordinates, left out of the line",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LineworkNote {
    pub id: EntityId,
    pub point_number: String,
    pub kind: LineworkNoteKind,
    /// The token, the missing number, the string name.
    pub detail: String,
}

/// One line built.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LineworkString {
    /// "WM01".
    pub name: String,
    /// "WM*" - see StringName.
    pub key: String,
    /// "01": what the key leaves of the name, or - for points and features
    /// that carry their string number apart from the code - that number as it
    /// is carried ("12" for code B in string 12, whose key is "B1*").
    pub number: String,
    /// In the order joined.
    pub points: Vec<EntityId>,
    /// The same, by number.
    pub point_numbers: Vec<String>,
    pub closed: bool,
    pub rectangle: bool,
    /// A "JPN" line rather than a run of the string.
    pub join: bool,
    /// Curves chorded into it.
    pub curves: usize,
    /// Vertices of the polyline, chords included.
    pub vertices: usize,
    pub layer: String,
    /// The line of an earlier run that this one is (the options'
    /// earlier_lines): kept as that entity and given these vertices, and
    /// `layer` is the layer it is on. INVALID_ENTITY_ID (0) for a line that is
    /// created.
    pub redrawn: EntityId,
}

/// What apply_survey_codes did to the lines a linework command created,
/// filled when the command first runs: the C++ `std::shared_ptr<const
/// SurveyCodingReport>`, shared with the command that fills it.
pub type SharedCodingReport = Arc<Mutex<SurveyCodingReport>>;

#[derive(Clone, Debug, Default)]
pub struct LineworkReport {
    /// The code property actually read.
    pub property: String,
    /// Point entities looked at.
    pub considered: usize,
    /// Entities asked about that are not points.
    pub not_points: usize,
    /// By name, then in the order built.
    pub strings: Vec<LineworkString>,
    /// LineworkOptions::skip_lines_already_drawn only: the lines planned that
    /// the drawing already holds and that are therefore not drawn again, in
    /// the order planned, each as `strings` would have listed it - but `layer`
    /// is the layer the line in the drawing is on.
    pub already_drawn: Vec<LineworkString>,
    /// In entity order.
    pub unplaced: Vec<UnplacedPoint>,
    pub notes: Vec<LineworkNote>,
    /// In name order.
    pub layers_created: Vec<String>,
    /// Points deleted because a run of their string replaced them (keep_points
    /// off); join targets no run placed are never among them.
    pub points_removed: usize,
    /// What apply_survey_codes did to the lines that were CREATED. It runs when
    /// the command first executes - it cannot plan styles for lines that do
    /// not exist yet - so this is empty until then, and filled by that
    /// execution. None when no line is created: a line redrawn as an earlier
    /// one keeps the style it has.
    pub styling: Option<SharedCodingReport>,
}

impl LineworkReport {
    /// A copy of `styling` as it stands: what the command's execution filled.
    pub fn styling_report(&self) -> Option<SurveyCodingReport> {
        self.styling.as_ref().map(|shared| lock(shared).clone())
    }
}

#[derive(Default)]
pub struct LineworkResult {
    /// ONE undoable command: layers, lines, their styling and (keep_points
    /// off) the removal of the points they replace. None when there is nothing
    /// to build - "nothing to do" and "failed" stay distinguishable, the
    /// contract apply_survey_codes and import_survey_project follow. Also None
    /// when every line planned is an earlier line that already has these
    /// vertices, or one the drawing already holds (skip_lines_already_drawn).
    pub command: Option<CommandPtr>,
    pub report: LineworkReport,
}

impl fmt::Debug for LineworkResult {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LineworkResult")
            .field(
                "command",
                &self.command.as_ref().map(|command| command.name()),
            )
            .field("report", &self.report)
            .finish()
    }
}

// ---- reading the points ----------------------------------------------------------

// The code a point carries, or nothing. Text only, as apply_survey_codes reads
// it: a number in the code property is a measurement named badly, not a code.
fn code_of<'a>(entity: &'a Entity, property: &str) -> Option<&'a String> {
    match entity.properties.get(property) {
        Some(PropertyValue::Text(text)) if !text.is_empty() => Some(text),
        _ => None,
    }
}

// The point number as text: survey import writes text ("102", "A12"), but a
// drawing from elsewhere may hold an integer, and both name the same point.
fn point_number_of(entity: &Entity, property: &str) -> String {
    match entity.properties.get(property) {
        None => String::new(),
        Some(PropertyValue::Text(text)) => String::from(text.trim_matches(ASCII_SPACE)),
        Some(PropertyValue::Boolean(_)) => String::new(), // a flag is not a number
        Some(value) => property_to_string(value),
    }
}

// How a point number sorts: numbers by value, so "9" comes before "10", and
// before every number that is not one, which sort by their text.
struct NumberKey<'a> {
    numeric: bool,
    value: f64,
    text: &'a str,
}

fn number_key(number: &str) -> NumberKey<'_> {
    match parse_finite_double(number) {
        Some(value) => NumberKey {
            numeric: true,
            value,
            text: number,
        },
        None => NumberKey {
            numeric: false,
            value: 0.0,
            text: number,
        },
    }
}

// The C++ NumberKey's operator<.
fn number_less(a: &NumberKey<'_>, b: &NumberKey<'_>) -> bool {
    if a.numeric != b.numeric {
        return a.numeric;
    }
    if a.numeric {
        a.value < b.value
    } else {
        a.text < b.text
    }
}

// A point that is going into a line.
#[derive(Clone, Debug, Default)]
struct Candidate {
    id: EntityId,
    at: Point2,
    height: Option<f64>,
    number: String,
    /// The code as the point carries it.
    code_text: String,
    code: FieldCode,
    /// The string number it carries beside its code (SURVEY_STRING_PROPERTY);
    /// empty when the code holds the whole string name.
    string: String,
    layer: String,
}

// What the rules MORE SPECIFIC than the bare "*" make of a string name: the
// first of them, most specific first, that says line or point decides. The
// fallback rule is left out on purpose - it answers every name, a typo
// included, so "it says line" is not a rule about THIS name.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum RuledAs {
    Line,
    Point,
    #[default]
    NoRule,
}

fn ruled_as(map: &SurveyMap, name: &str) -> RuledAs {
    let mut any = false;
    for rule in map.r#match(name) {
        if rule.key == "*" {
            break; // most specific first: only the fallback is left
        }
        any = true;
        if let Some(breakline) = rule.breakline {
            return if breakline == SurveyBreakline::Line {
                RuledAs::Line
            } else {
                RuledAs::Point
            };
        }
    }
    // Rules that never say it is a line leave it a point, as process_linework
    // has always read a resolved breakline that is unset.
    if any { RuledAs::Point } else { RuledAs::NoRule }
}

// A stretch of one string between its start and its end.
#[derive(Default)]
struct Run<'a> {
    points: Vec<&'a Candidate>,
    closed: bool,
    rectangle: bool,
}

// The polyline a run becomes, before it is an entity.
#[derive(Default)]
struct Shape {
    vertices: Vec<Point2>,
    heights: Vec<Option<f64>>,
    closed: bool,
    rectangle: bool,
    curves: usize,
}

impl Shape {
    fn add(&mut self, at: Point2, height: Option<f64>) {
        self.vertices.push(at);
        self.heights.push(height);
    }

    fn length(&self) -> f64 {
        Polyline2::new(self.vertices.clone(), self.closed).length()
    }
}

fn note(
    notes: &mut Vec<LineworkNote>,
    at: &Candidate,
    kind: LineworkNoteKind,
    detail: impl Into<String>,
) {
    notes.push(LineworkNote {
        id: at.id,
        point_number: at.number.clone(),
        kind,
        detail: detail.into(),
    });
}

fn unplace(report: &mut LineworkReport, point: &Candidate, reason: UnplacedReason) {
    report.unplaced.push(UnplacedPoint {
        id: point.id,
        point_number: point.number.clone(),
        code: point.code_text.clone(),
        reason,
    });
}

// ---- curves ----------------------------------------------------------------------

// Appends the arc of `arc`'s circle from `from` to `to`, travelling in
// `arc`'s direction, as chords no further than `tolerance` from it. `from` is
// already in the shape; `to` is appended exactly, so a surveyed point stays a
// vertex where it was measured.
//
// The chord of angle t on radius r stands off the arc by its sagitta,
// r(1 - cos(t/2)); keeping that within the tolerance gives the largest step
// t = 2 acos(1 - tolerance / r). Heights between two surveyed heights are
// interpolated along the arc, which is what a kerb or a channel between two
// shots is assumed to do; with either end unsurveyed the chords have none.
fn append_arc(shape: &mut Shape, arc: &Arc2, from: &Candidate, to: &Candidate, tolerance: f64) {
    let start_angle = (from.at - arc.center).angle();
    let end_angle = (to.at - arc.center).angle();
    let delta = if arc.sweep >= 0.0 {
        normalize_angle(end_angle - start_angle)
    } else {
        -normalize_angle(start_angle - end_angle)
    };
    let ratio = cpp::max(-1.0, 1.0 - tolerance / arc.radius);
    let step = 2.0 * ratio.acos();
    // A count too large for an integer is undefined in the C++ cast and
    // saturates here; neither can be reached by a finite positive tolerance
    // on a real survey.
    let chords = cpp::max(1.0, (delta.abs() / step).ceil()) as usize;
    for i in 1..chords {
        let t = i as f64 / chords as f64;
        let height = match (from.height, to.height) {
            (Some(first), Some(second)) => Some(lerp(first, second, t)),
            _ => None,
        };
        shape.add(arc.point_at_angle(start_angle + delta * t), height);
    }
    shape.add(to.at, to.height);
}

// One arc of a curve: the circle through points[a], points[b] and points[c],
// chorded from segment `segment_from` to the end at points[c]. True when it
// was a circle (and so chorded); points in a straight line are joined
// straight, with a note.
#[allow(
    clippy::too_many_arguments,
    reason = "The C++ lambda `piece` and what it captured: the shape, the points, the tolerance and the notes."
)]
fn curve_piece(
    shape: &mut Shape,
    points: &[&Candidate],
    a: usize,
    b: usize,
    c: usize,
    segment_from: usize,
    tolerance: f64,
    notes: &mut Vec<LineworkNote>,
) -> bool {
    let arc = Arc2::through_points(points[a].at, points[b].at, points[c].at);
    for i in segment_from..c {
        match &arc {
            None => shape.add(points[i + 1].at, points[i + 1].height),
            Some(arc) => append_arc(shape, arc, points[i], points[i + 1], tolerance),
        }
    }
    if arc.is_none() {
        note(notes, points[a], LineworkNoteKind::CurveCollinear, "");
        false
    } else {
        true
    }
}

// Appends the curve through points[first..last] (both in the shape's run;
// points[first] already added).
fn append_curve(
    shape: &mut Shape,
    points: &[&Candidate],
    first: usize,
    last: usize,
    tolerance: f64,
    notes: &mut Vec<LineworkNote>,
) {
    let count = last - first; // segments in the curve
    if count < 2 {
        // Two points are on every circle through them: there is no curve to
        // draw, only the straight line the surveyor may not have meant.
        note(notes, points[first], LineworkNoteKind::CurveTooShort, "");
        shape.add(points[last].at, points[last].height);
        return;
    }
    // Each consecutive three points define an arc; a segment left over at the
    // end lies on the arc through the last three. The curve counts as one
    // only if some piece of it was chorded: points in a straight line are
    // joined straight, and the report must not call that a curve.
    let mut curved = false;
    let mut k = first;
    while k < last {
        if k + 2 <= last {
            curved |= curve_piece(shape, points, k, k + 1, k + 2, k, tolerance, notes);
            k += 2;
        } else {
            curved |= curve_piece(
                shape,
                points,
                last - 2,
                last - 1,
                last,
                last - 1,
                tolerance,
                notes,
            );
            k = last;
        }
    }
    if curved {
        shape.curves += 1;
    }
}

// The polyline for one run, with its curves, its closure or its rectangle.
fn shape_of(run: &Run<'_>, tolerance: f64, notes: &mut Vec<LineworkNote>) -> Shape {
    let mut shape = Shape::default();
    let points = &run.points;
    shape.closed = run.closed;
    let last_point = points[points.len() - 1];

    if run.rectangle {
        if points.len() == 3 {
            // Side p0-p1, reaching p2's side of it by p2's distance from it.
            let side = points[1].at - points[0].at;
            let length = side.length();
            let normal = if length > tolerance::GEOMETRIC {
                side.perpendicular() / length
            } else {
                Vec2::new(0.0, 0.0)
            };
            let width = normal.dot(points[2].at - points[0].at);
            if width.abs() > tolerance::GEOMETRIC {
                shape.rectangle = true;
                shape.closed = true;
                shape.add(points[0].at, points[0].height);
                shape.add(points[1].at, points[1].height);
                // Constructed, not surveyed: no height rather than a guess.
                shape.add(points[1].at + normal * width, None);
                shape.add(points[0].at + normal * width, None);
                return shape;
            }
            // The third point is on the first side: a line, not an area.
            note(
                notes,
                last_point,
                LineworkNoteKind::RectangleShape,
                "no width: drawn open",
            );
            shape.closed = false;
        } else {
            // Not three points, so there is no rectangle to construct; the
            // points themselves are the best account of the area, closed
            // when they can enclose one. The detail says which, because the
            // note is what tells the surveyor what became of their RECT.
            shape.closed = points.len() >= 3;
            note(
                notes,
                last_point,
                LineworkNoteKind::RectangleShape,
                format!(
                    "{}{}",
                    points.len(),
                    if shape.closed {
                        " points: drawn closed through them"
                    } else {
                        " points: drawn open"
                    }
                ),
            );
        }
    }
    if shape.closed && points.len() < 3 {
        // Two points closed back on themselves enclose nothing.
        note(notes, last_point, LineworkNoteKind::CloseTooShort, "");
        shape.closed = false;
    }

    // Where the curves are: from a point carrying arcStart to the next
    // carrying arcEnd. A point carrying both ends one and begins the next.
    let mut curves: Vec<(usize, usize)> = Vec::new();
    let mut open: Option<usize> = None;
    for (i, point) in points.iter().enumerate() {
        let code = &point.code;
        if code.has(LineworkControl::ArcEnd) {
            match open.take() {
                Some(start) => curves.push((start, i)),
                None => note(notes, point, LineworkNoteKind::CurveEndWithoutStart, ""),
            }
        }
        if code.has(LineworkControl::ArcStart) {
            if let Some(start) = open {
                // A new curve begins where the last one ends.
                curves.push((start, i));
            }
            open = Some(i);
        }
    }
    if let Some(start) = open {
        // Taken to end at the string's last point, as the likeliest place the
        // surveyor meant; begun ON that point, there is nothing to curve.
        if start + 1 < points.len() {
            note(
                notes,
                points[start],
                LineworkNoteKind::CurveUnterminated,
                "taken to end at the string's last point",
            );
            curves.push((start, points.len() - 1));
        } else {
            note(
                notes,
                points[start],
                LineworkNoteKind::CurveUnterminated,
                "begun on the string's last point: nothing curved",
            );
        }
    }

    shape.add(points[0].at, points[0].height);
    let mut i = 0;
    let mut curve = 0;
    while i + 1 < points.len() {
        if curve < curves.len() && curves[curve].0 == i {
            let (first, last) = curves[curve];
            append_curve(&mut shape, points, first, last, tolerance, notes);
            i = last;
            curve += 1;
            continue;
        }
        shape.add(points[i + 1].at, points[i + 1].height);
        i += 1;
    }
    shape
}

// Splits one string's points, in order, into runs at its start, end, close
// and rectangle codes.
fn runs_of(ordered: &[Candidate]) -> Vec<Run<'_>> {
    let mut runs = Vec::new();
    let mut current: Option<Run<'_>> = None;
    for point in ordered {
        if point.code.has(LineworkControl::Start)
            && let Some(run) = current.take()
        {
            runs.push(run);
        }
        let run = current.get_or_insert_with(Run::default);
        run.points.push(point);
        if point.code.has(LineworkControl::Rectangle) {
            run.rectangle = true;
            runs.extend(current.take());
        } else if point.code.has(LineworkControl::Close) {
            run.closed = true;
            runs.extend(current.take());
        } else if point.code.has(LineworkControl::End) {
            runs.extend(current.take());
        }
    }
    runs.extend(current);
    runs
}

// ---- styling, once the lines exist -----------------------------------------------

// The entities a step created, kept where the step after it in the same
// transaction can read them: the C++ StyleLinesCommand held a reference to
// the create command beside it, which a Rust transaction's parts cannot hold.
type CreatedLines = Arc<Mutex<Vec<EntityId>>>;

// The create step, saying what it created each time it runs.
struct RecordCreated {
    inner: CommandPtr,
    created: CreatedLines,
}

impl Command for RecordCreated {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn is_destructive(&self) -> bool {
        self.inner.is_destructive()
    }

    fn validate(&self, context: &CommandContext<'_>) -> Status {
        self.inner.validate(context)
    }

    fn execute(&mut self, context: &mut CommandContext<'_>) -> Status {
        self.inner.execute(context)?;
        *lock(&self.created) = self.inner.created_entities();
        Ok(())
    }

    fn undo(&mut self, context: &mut CommandContext<'_>) -> Status {
        self.inner.undo(context)
    }

    fn redo(&mut self, context: &mut CommandContext<'_>) -> Status {
        self.inner.redo(context)?;
        *lock(&self.created) = self.inner.created_entities();
        Ok(())
    }

    fn created_entities(&self) -> Vec<EntityId> {
        self.inner.created_entities()
    }
}

// Styles the lines another part of the same transaction has just created, by
// planning apply_survey_codes against the document at that moment - the only
// moment the lines exist to be planned for. Redo replays what was planned, so
// the ids and styles come back exactly. It reaches the document's survey map
// and library through the state the document lends its steps (StateLink), so
// run through any other document's stack it finds nothing lent and refuses:
// the C++ compared the context's model with its document's.
struct StyleLinesCommand {
    link: StateLink,
    lines: CreatedLines,
    options: SurveyCodingOptions,
    report: SharedCodingReport,
    inner: Option<CommandPtr>,
}

impl Command for StyleLinesCommand {
    fn name(&self) -> &str {
        "STYLE_LINEWORK"
    }

    fn validate(&self, _context: &CommandContext<'_>) -> Status {
        // It plans against its Document; run on another model it would style
        // whatever that document's entities of the same ids are.
        self.link.with(|_| ()).map_err(|_| {
            Error::new(
                ErrorCode::InvalidState,
                "linework was planned for another document",
            )
        })
    }

    fn execute(&mut self, context: &mut CommandContext<'_>) -> Status {
        self.options.ids = lock(&self.lines).clone();
        // An empty id list means EVERY entity to apply_survey_codes; with no
        // lines there is nothing to style, not everything.
        if self.options.ids.is_empty() {
            return Ok(());
        }
        let planned = {
            let model: &Model = &*context.model;
            let options = &self.options;
            let mut report = lock(&self.report);
            self.link.with(|state| {
                apply_survey_codes_to(
                    CodingSource {
                        model,
                        survey_map: &state.survey_map,
                        library: &state.library,
                    },
                    options,
                    Some(&mut *report),
                )
            })?
        };
        self.inner = planned?;
        let Some(inner) = &mut self.inner else {
            return Ok(());
        };
        inner.validate(context)?;
        inner.execute(context)
    }

    fn undo(&mut self, context: &mut CommandContext<'_>) -> Status {
        match &mut self.inner {
            Some(inner) => inner.undo(context),
            None => Ok(()),
        }
    }

    fn redo(&mut self, context: &mut CommandContext<'_>) -> Status {
        match &mut self.inner {
            Some(inner) => inner.redo(context),
            None => Ok(()),
        }
    }
}

// The code property to read when none is named: the first candidate any of
// these points carries, as apply_survey_codes chooses among all entities.
fn points_code_property(points: &[&Entity]) -> String {
    for &candidate in code_property_candidates() {
        if points
            .iter()
            .any(|point| code_of(point, candidate).is_some())
        {
            return String::from(candidate);
        }
    }
    String::from(code_property_candidates()[0])
}

// The point as a candidate for a line: where it is, how high, its number.
fn candidate_of(entity: &Entity, number_property: &str) -> Candidate {
    let at = match &entity.geometry {
        Geometry::Point(point) => point.position,
        _ => Point2::default(),
    };
    Candidate {
        id: entity.id,
        at,
        height: heights_of(&entity.properties, 1).first().copied().flatten(),
        number: point_number_of(entity, number_property),
        layer: entity.layer.clone(),
        ..Candidate::default()
    }
}

// Every point in the drawing by its number. Several under one number are kept
// as several, so that a caller can refuse to guess between them.
fn points_by_number<'a>(
    model: &'a Model,
    number_property: &str,
) -> BTreeMap<String, Vec<&'a Entity>> {
    let mut by_number: BTreeMap<String, Vec<&'a Entity>> = BTreeMap::new();
    for id in model.entities.ids() {
        let Some(entity) = model.entities.find(id) else {
            continue;
        };
        if matches!(entity.geometry, Geometry::Point(_)) {
            let number = point_number_of(entity, number_property);
            if !number.is_empty() {
                by_number.entry(number).or_default().push(entity);
            }
        }
    }
    by_number
}

// What makes a line the line of ONE string, here and in the run that drew it
// before: the code it carries and its string number.
type Identity = (String, String);

// What joining coded points and drawing survey features share: the layer a
// line goes on, the line itself and its entry in the report, and the ONE
// command that builds them all.
struct LineBuilder<'a> {
    model: &'a Model,
    link: StateLink,
    property: String,
    create_layers: bool,
    report: LineworkReport,
    lines: Vec<Entity>,
    // The earlier run's lines not yet taken, earliest first, and those taken
    // that this run moves, as they are to be.
    earlier: BTreeMap<Identity, VecDeque<&'a Entity>>,
    redrawn: Vec<Entity>,
    // skip_lines_already_drawn: the drawing's coded polylines, in entity order.
    drawn: BTreeMap<Identity, Vec<&'a Entity>>,
    placed: BTreeSet<EntityId>,
    run_members: BTreeSet<EntityId>,
    layers_needed: BTreeSet<String>,
}

impl<'a> LineBuilder<'a> {
    // An empty `property` means the first of code_property_candidates(), as it
    // does to apply_survey_codes when no entity carries one: a line needs its
    // code under SOME name, because that is what the rules style it by, and a
    // property with an empty name is one the rest of Katana refuses. The name
    // used is the report's `property`, so the two cannot disagree.
    //
    // `earlier_lines` are the lines an earlier run drew of the same strings
    // (LineworkOptions::earlier_lines): filed here by what makes a line the
    // line of one string, earliest first whatever order they were given in.
    //
    // `skip_drawn` is LineworkOptions::skip_lines_already_drawn: every coded
    // polyline of the drawing is then filed the same way, to be compared with
    // each line before it is drawn.
    fn new(
        source: CodingSource<'a>,
        link: StateLink,
        property: &str,
        create_layers: bool,
        earlier_lines: &[EntityId],
        skip_drawn: bool,
        mut report: LineworkReport,
    ) -> Self {
        let property = if property.is_empty() {
            String::from(code_property_candidates()[0])
        } else {
            String::from(property)
        };
        report.property.clone_from(&property);
        let model = source.model;
        let mut builder = Self {
            model,
            link,
            property,
            create_layers,
            report,
            lines: Vec::new(),
            earlier: BTreeMap::new(),
            redrawn: Vec::new(),
            drawn: BTreeMap::new(),
            placed: BTreeSet::new(),
            run_members: BTreeSet::new(),
            layers_needed: BTreeSet::new(),
        };
        let mut ids = earlier_lines.to_vec();
        ids.sort_unstable();
        ids.dedup();
        for id in ids {
            // Only a polyline: what a line drawn here is. One the person has
            // since made something else of is no longer a line to redraw.
            if let Some(line) = model.entities.find(id)
                && matches!(line.geometry, Geometry::Polyline(_))
            {
                let identity = builder.identity_of(line);
                builder.earlier.entry(identity).or_default().push_back(line);
            }
        }
        if skip_drawn {
            // Only the ones that carry a code: a line drawn here always does,
            // and most of a drawing's polylines carry none.
            for id in model.entities.ids() {
                if let Some(entity) = model.entities.find(id)
                    && matches!(entity.geometry, Geometry::Polyline(_))
                    && code_of(entity, &builder.property).is_some()
                {
                    let identity = builder.identity_of(entity);
                    builder.drawn.entry(identity).or_default().push(entity);
                }
            }
        }
        builder
    }

    fn identity_of(&self, line: &Entity) -> Identity {
        (
            code_of(line, &self.property).cloned().unwrap_or_default(),
            String::from(survey_string_of(line)),
        )
    }

    // A rule's model is Katana's layer. With none - or one that is missing and
    // may not be created - the line stays with its points, on `fallback`.
    // Deciding is not creating: a layer is created only if a line goes on it.
    fn layer_for(&self, found: &SurveyMatch, code: &str, fallback: &str) -> Result<String> {
        let model = &found.resolved.model;
        if model.is_empty() {
            return Ok(String::from(fallback));
        }
        if let Err(status) = validate_layer_path(model) {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "the survey map gives a code a model that is not a valid layer name",
                format!("code={code} model={model} {}", status.context),
            ));
        }
        if !self.model.layers.contains(model) && !self.create_layers {
            return Ok(String::from(fallback));
        }
        Ok(model.clone())
    }

    // The polyline of the drawing that `line`, as it is about to be drawn,
    // already is (LineworkOptions::skip_lines_already_drawn): one of its code
    // and string number that runs through the same vertices at the same
    // heights, closed alike. None when there is none, and always when the
    // option is off. Compared exactly, with no tolerance: a line is built
    // from its points' own coordinates each time, so one that differs in the
    // last place is the line of a point that has moved. Heights are read
    // back through the one reader on both sides, so a height the writer does
    // not keep (a non-finite one) compares as the absence it becomes.
    fn drawn_already(&self, line: &Entity) -> Option<&'a Entity> {
        let found = self.drawn.get(&self.identity_of(line))?;
        let vertices = match &line.geometry {
            Geometry::Polyline(polyline) => polyline.vertices.len(),
            _ => 0,
        };
        let heights = heights_of(&line.properties, vertices);
        found.iter().copied().find(|drawn| {
            drawn.geometry == line.geometry && heights_of(&drawn.properties, vertices) == heights
        })
    }

    // `code` is what the line carries in the code property, so that
    // apply_survey_codes - now, and whenever it is applied again - codes the
    // line as its points were coded. `texts` are further text properties
    // (a survey feature's description); an empty value is not written.
    fn emit(
        &mut self,
        mut built: LineworkString,
        code: &str,
        shape: &Shape,
        members: &[&Candidate],
        texts: &[(&str, &str)],
    ) {
        let mut entity = Entity {
            geometry: Geometry::Polyline(Polyline2::new(shape.vertices.clone(), shape.closed)),
            layer: built.layer.clone(),
            ..Entity::default()
        };
        // An empty code is no code (code_of reads it so), not a code "".
        if !code.is_empty() {
            entity
                .properties
                .insert(self.property.clone(), PropertyValue::from(code));
        }
        for &(key, value) in texts {
            if !key.is_empty() && !value.is_empty() {
                entity
                    .properties
                    .insert(String::from(key), PropertyValue::from(value));
            }
        }
        set_heights(&mut entity.properties, &shape.heights);

        let identity = self.identity_of(&entity);
        let earlier = self
            .earlier
            .get_mut(&identity)
            .and_then(|lines| lines.pop_front());
        let mut standing = false; // the drawing holds this very line already
        if let Some(was) = earlier {
            // The line of this string that an earlier run drew: the same
            // entity, moved. Deleting it and creating this one would take
            // every label, leader and dimension on it away with it, and what
            // the person has since given it - a layer, a style - as well, as
            // a re-adjustment never does to a point either. Only where it
            // runs, and how high, is this run's to say.
            let mut redrawn = was.clone();
            redrawn.geometry = entity.geometry;
            set_heights(&mut redrawn.properties, &shape.heights);
            built.layer.clone_from(&was.layer);
            built.redrawn = was.id;
            if redrawn != *was {
                // One that already runs there is not touched.
                self.redrawn.push(redrawn);
            }
        } else if let Some(drawn) = self.drawn_already(&entity) {
            // A second line on top of it would be nobody's and look like
            // nothing: it is reported where it stands, and nothing is drawn
            // or created for it.
            built.layer.clone_from(&drawn.layer);
            standing = true;
        } else {
            self.lines.push(entity);
            if !self.model.layers.contains(&built.layer) {
                self.layers_needed.insert(built.layer.clone());
            }
        }

        built.closed = shape.closed;
        built.rectangle = shape.rectangle;
        built.curves = shape.curves;
        built.vertices = shape.vertices.len();
        for member in members {
            // A survey feature's points are the project's, not entities.
            if member.id != INVALID_ENTITY_ID {
                built.points.push(member.id);
                self.placed.insert(member.id);
                // A line this run did not draw stands in for no point: its
                // points are in a line, and are never removed on its account.
                if !built.join && !standing {
                    self.run_members.insert(member.id);
                }
            }
            built.point_numbers.push(member.number.clone());
        }
        if standing {
            self.report.already_drawn.push(built);
        } else {
            self.report.strings.push(built);
        }
    }

    // The points some run of their own string is drawn through - the only
    // points a line stands in for. A point reached only by a join (a tree, an
    // uncoded control point) is in a line but is not replaced by one: its own
    // code, number and symbol are still only on it.
    fn run_members(&self) -> Vec<EntityId> {
        self.run_members.iter().copied().collect()
    }

    // Layers, the new lines and their styling, the earlier lines given their
    // new vertices, and the removal of `remove`, as one command; None when
    // there is none of them - no line was built, or every line built is an
    // earlier one that already runs where this run puts it.
    fn finish(
        mut self,
        name: &str,
        mut coding: SurveyCodingOptions,
        remove: Vec<EntityId>,
    ) -> (Option<CommandPtr>, LineworkReport) {
        // A point a line reached is in a line, whatever its own run made of
        // it: a lone point, or a point code, that a join reached.
        let placed = &self.placed;
        self.report
            .unplaced
            .retain(|point| !placed.contains(&point.id));
        self.report.unplaced.sort_by_key(|point| point.id);
        if self.lines.is_empty() && self.redrawn.is_empty() && remove.is_empty() {
            return (None, self.report);
        }
        self.report.layers_created = self.layers_needed.iter().cloned().collect();

        let mut transaction = Transaction::new(name);
        for layer_name in &self.report.layers_created {
            transaction.add(create_layer(Layer {
                name: layer_name.clone(),
                ..Layer::default()
            }));
        }
        if !self.lines.is_empty() {
            let created: CreatedLines = Arc::default();
            transaction.add(Box::new(RecordCreated {
                inner: create_entities(std::mem::take(&mut self.lines)),
                created: Arc::clone(&created),
            }));

            let styling: SharedCodingReport = Arc::default();
            self.report.styling = Some(Arc::clone(&styling));
            coding.property.clone_from(&self.property);
            transaction.add(Box::new(StyleLinesCommand {
                link: self.link.clone(),
                lines: created,
                options: coding,
                report: styling,
                inner: None,
            }));
        }
        if !self.redrawn.is_empty() {
            let change = ChangeSet {
                modify: std::mem::take(&mut self.redrawn),
                ..ChangeSet::default()
            };
            transaction.add(Box::new(ChangeSetCommand::new(
                "REDRAW_LINEWORK",
                move |_: &CommandContext<'_>| Ok(change.clone()),
                false,
            )));
        }

        self.report.points_removed = remove.len();
        if !remove.is_empty() {
            transaction.add(delete_entities(remove));
        }
        let command: CommandPtr = Box::new(transaction);
        (Some(command), self.report)
    }
}

// What a string name resolves to: its code and, where the points carry one
// beside it, their string number - the first point of the name, in entity
// order, says which - and that pair is what is looked up (survey_lookup_name)
// and what the line then carries, so the line finds the rule its points found.
struct Named {
    code: String,
    string: String,
    lookup: String,
    found: SurveyMatch,
    /// Asked only under only_ruled_lines.
    ruled: RuledAs,
}

/// Plans the linework for `document`. Fails on invalid options (control codes
/// spelled ambiguously, a tolerance that is not a positive finite number) or a
/// rule model that is not a valid layer name - never on a point it cannot
/// place, which is reported instead.
///
/// The command must be executed on THIS document: it styles the lines by
/// planning apply_survey_codes against it when it runs, and refuses any other.
///
/// Only a point is a candidate: its code's resolved breakline is Line, or it
/// carries a control code. Its string name groups it; the order orders it;
/// "ST" begins a new line at it, "END" ends the line at it, "CL" ends and
/// closes, "RECT" ends it as a rectangle. The line goes on the rule's model
/// (a rule's model is Katana's layer), or the first point's layer when the rule
/// names none.
///
/// The line carries what its points carry: the string name in the code
/// property, or - when the points keep their string number apart - the code
/// there and the number under SURVEY_STRING_PROPERTY, so that
/// apply_survey_codes finds the rule for the line that it found for the points.
///
/// Curves. "BC" begins a curve at a point and "EC" ends it; each consecutive
/// three curve points define an arc ((p0,p1,p2), (p2,p3,p4), ...), and when one
/// segment is left over it lies on the arc through the last three. Two points
/// cannot define a curve and are joined straight, with a note.
///
/// Rectangles. Three points p0, p1, p2 with "RECT" make the closed rectangle
/// with side p0-p1, reaching p2's side of it by p2's distance from that side.
/// The two constructed corners have no height: they were not surveyed.
pub fn process_linework(document: &Document, options: &LineworkOptions) -> Result<LineworkResult> {
    process_linework_to(CodingSource::of(document), document.state_link(), options)
}

/// process_linework over the parts it reads, with the link to the state of
/// the document it is for: what a step of the document's own command calls while the
/// document's state is lent to it (the survey import's finish).
pub(crate) fn process_linework_to(
    source: CodingSource<'_>,
    link: StateLink,
    options: &LineworkOptions,
) -> Result<LineworkResult> {
    validate(&options.codes)?;
    if !(options.chord_tolerance.is_finite() && options.chord_tolerance > 0.0) {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "the chord tolerance must be a positive distance",
            // Exact, and the same in every locale: a fixed format would print
            // 1e-9 as "0.000000".
            format!(
                "chordTolerance={}",
                format_exact_real(options.chord_tolerance)
            ),
        ));
    }

    let model = source.model;
    let map = source.survey_map;
    let mut report = LineworkReport::default();

    // In entity order whatever order they were given in: that IS the entity
    // order a string may be joined in, and it makes the report deterministic.
    let mut subject = if options.ids.is_empty() {
        model.entities.ids()
    } else {
        options.ids.clone()
    };
    subject.sort_unstable();
    subject.dedup();

    let mut points: Vec<&Entity> = Vec::new();
    for &id in &subject {
        match model.entities.find(id) {
            Some(entity) if matches!(entity.geometry, Geometry::Point(_)) => points.push(entity),
            _ => report.not_points += 1,
        }
    }
    let property = if options.property.is_empty() {
        points_code_property(&points)
    } else {
        options.property.clone()
    };

    // Resolved once per string name rather than once per point: a survey of
    // 30,000 points has a few hundred names, and lookup walks every rule.
    let mut resolved: BTreeMap<String, Named> = BTreeMap::new();
    let mut strings: BTreeMap<String, Vec<Candidate>> = BTreeMap::new(); // by name
    for &entity in &points {
        report.considered += 1;
        let mut candidate = candidate_of(entity, &options.point_number_property);
        candidate.code_text = code_of(entity, &property).cloned().unwrap_or_default();
        candidate.code = parse_field_code(&candidate.code_text, &options.codes);
        candidate.string = String::from(survey_string_of(entity));
        if candidate.code.name.is_empty() {
            unplace(&mut report, &candidate, UnplacedReason::NoCode);
            continue;
        }
        for token in &candidate.code.unknown_tokens {
            note(
                &mut report.notes,
                &candidate,
                LineworkNoteKind::UnknownToken,
                token.as_str(),
            );
        }
        if candidate.code.join_without_target {
            note(
                &mut report.notes,
                &candidate,
                LineworkNoteKind::JoinWithoutTarget,
                "",
            );
        }

        // The string name: the code's first token, followed by the string
        // number the point carries beside it when it carries one.
        let name = format!("{}{}", candidate.code.name, candidate.string);
        let named = resolved.entry(name.clone()).or_insert_with(|| {
            let lookup = survey_lookup_name(map, &candidate.code.name, &candidate.string);
            let found = map.lookup(&lookup);
            let ruled = if options.only_ruled_lines {
                ruled_as(map, &lookup)
            } else {
                RuledAs::NoRule
            };
            Named {
                code: candidate.code.name.clone(),
                string: candidate.string.clone(),
                lookup,
                found,
                ruled,
            }
        });
        let line = if options.only_ruled_lines {
            named.ruled == RuledAs::Line
        } else {
            named.found.resolved.breakline == Some(SurveyBreakline::Line)
                || !candidate.code.controls.is_empty()
        };
        if !line {
            let no_rule = if options.only_ruled_lines {
                named.ruled == RuledAs::NoRule
            } else {
                named.found.empty()
            };
            unplace(
                &mut report,
                &candidate,
                if no_rule {
                    UnplacedReason::NoRule
                } else {
                    UnplacedReason::PointCode
                },
            );
            continue;
        }
        if options.order == LineworkOrder::PointNumber && candidate.number.is_empty() {
            unplace(&mut report, &candidate, UnplacedReason::NoPointNumber);
            continue;
        }
        strings.entry(name).or_default().push(candidate);
    }

    // Every point in the drawing by number, for joins: a join may reach a
    // point outside the selection, because the code says so.
    let any_join = strings
        .values()
        .any(|candidates| candidates.iter().any(|c| !c.code.join_to.is_empty()));
    let by_number = if any_join {
        points_by_number(model, &options.point_number_property)
    } else {
        BTreeMap::new()
    };

    let mut builder = LineBuilder::new(
        source,
        link,
        &property,
        options.coding.create_layers,
        &options.earlier_lines,
        options.skip_lines_already_drawn,
        report,
    );

    for (name, candidates) in &mut strings {
        if options.order == LineworkOrder::PointNumber {
            // Stable, so equal numbers keep entity order.
            candidates.sort_by(|a, b| {
                let (left, right) = (number_key(&a.number), number_key(&b.number));
                if number_less(&left, &right) {
                    std::cmp::Ordering::Less
                } else if number_less(&right, &left) {
                    std::cmp::Ordering::Greater
                } else {
                    std::cmp::Ordering::Equal
                }
            });
            for pair in candidates.windows(2) {
                if pair[1].number == pair[0].number {
                    note(
                        &mut builder.report.notes,
                        &pair[1],
                        LineworkNoteKind::DuplicatePointNumber,
                        pair[1].number.as_str(),
                    );
                }
            }
        }
        let candidates: &[Candidate] = candidates;

        // Resolved when its first point was read.
        let Some(named) = resolved.get(name) else {
            continue;
        };
        let split = split_string_name(map, &named.lookup);
        if !split.matched {
            note(
                &mut builder.report.notes,
                &candidates[0],
                LineworkNoteKind::NoRuleForName,
                name.as_str(),
            );
        } else if split.fallback_only {
            note(
                &mut builder.report.notes,
                &candidates[0],
                LineworkNoteKind::FallbackOnlyName,
                name.as_str(),
            );
        }
        let layer = builder.layer_for(&named.found, name, &candidates[0].layer)?;
        let header = LineworkString {
            name: name.clone(),
            key: split.key,
            // A number carried apart is the number: "B" in string "12" is
            // keyed by "B1*", and what that key leaves of the name is "2".
            number: if named.string.is_empty() {
                split.number
            } else {
                named.string.clone()
            },
            layer,
            ..LineworkString::default()
        };
        // An empty value is not written, so points that hold the whole name
        // in their code make a line that does too, as before.
        let carried = [(SURVEY_STRING_PROPERTY, named.string.as_str())];

        for run in runs_of(candidates) {
            if run.points.len() < 2 {
                unplace(
                    &mut builder.report,
                    run.points[0],
                    UnplacedReason::LonePoint,
                );
                continue;
            }
            let shape = shape_of(&run, options.chord_tolerance, &mut builder.report.notes);
            if !(shape.length() > tolerance::GEOMETRIC) {
                for &member in &run.points {
                    unplace(&mut builder.report, member, UnplacedReason::Coincident);
                }
                continue;
            }
            builder.emit(header.clone(), &named.code, &shape, &run.points, &carried);
        }

        for from in candidates {
            if from.code.join_to.is_empty() {
                continue;
            }
            let target = match by_number.get(&from.code.join_to) {
                Some(targets) if targets.len() == 1 && targets[0].id != from.id => targets[0],
                _ => {
                    note(
                        &mut builder.report.notes,
                        from,
                        LineworkNoteKind::JoinTargetMissing,
                        from.code.join_to.as_str(),
                    );
                    continue;
                }
            };
            let to = candidate_of(target, &options.point_number_property);
            if !(from.at.distance_to(to.at) > tolerance::GEOMETRIC) {
                note(
                    &mut builder.report.notes,
                    from,
                    LineworkNoteKind::JoinTargetMissing,
                    format!("{} is in the same place", from.code.join_to),
                );
                continue;
            }
            let mut shape = Shape::default();
            shape.add(from.at, from.height);
            shape.add(to.at, to.height);
            let joined = LineworkString {
                join: true,
                ..header.clone()
            };
            builder.emit(joined, &named.code, &shape, &[from, &to], &carried);
        }
    }

    let remove = if options.keep_points {
        Vec::new()
    } else {
        // Only the points a run of their own string replaced. A join target
        // no run placed is kept, in the selection or out of it: the join line
        // does not carry its code, its number or its symbol. Runs are built
        // from the selection alone, so this never reaches outside it.
        builder.run_members()
    };
    // None when there is nothing to build, and no error.
    let (command, report) = builder.finish("PROCESS_LINEWORK", options.coding.clone(), remove);
    Ok(LineworkResult { command, report })
}

// ---- survey features ---------------------------------------------------------------

/// Some field files string their points themselves - a LandXML PlanFeature, a
/// controller's line record - and those arrive as the survey's SurveyFeature: a
/// code, the point ids in observation order, and whether it closes. There the
/// FILE has said which points are one line, so this draws exactly that, and
/// the code table decides only where each line goes and how it looks, as it
/// does for process_linework's lines (the same layer choice, the same styling
/// by apply_survey_codes when the command runs).
///
/// Use this OR process_linework on one set of points, not both: a kerb the file
/// strung and whose points are also coded KB1 would be drawn twice.
#[derive(Clone, Debug, Default)]
pub struct SurveyFeatureOptions {
    /// How the project was imported: the code and description properties the
    /// lines carry (import.code_property, import.description_property), and
    /// the layer its points went on (layer_for_point), which is where a line
    /// goes when its code names no model - with its points, as
    /// process_linework's does. Pass the options the import was given.
    /// import.create_layers is NOT read: coding.create_layers governs every
    /// layer here, as it does there. An EMPTY import.code_property (the import
    /// wrote no code) still gives each line its code, under
    /// code_property_candidates()[0], which the report names: the rules style
    /// a line by the code it carries.
    pub import: SurveyImportOptions,
    /// As LineworkOptions::coding: how the lines are styled, and whether a
    /// missing layer may be created.
    pub coding: SurveyCodingOptions,
    /// On, the code table decides WHETHER a feature is a line as well as how
    /// it looks, and the feature is read as a field file means it - its code
    /// and, in `name`, its string number. It is drawn only when a rule MORE
    /// SPECIFIC than the bare "*" makes its string name (survey_lookup_name of
    /// code and name) a line; the others are in `unplaced` as NoRule or
    /// PointCode. Some readers make a feature of EVERY coded shot, so without
    /// this every survey mark of one code is joined to the next. The line
    /// carries the code in the code property and the name under
    /// SURVEY_STRING_PROPERTY, as its points do, and is reported as the two
    /// ("KB 32", number "32"), not by the number alone. Off, as it always was:
    /// every feature with two positioned points is drawn, named by its name,
    /// and its code alone is looked up.
    pub only_ruled_lines: bool,
    /// WHICH features, when not all of them: one entry per feature of the
    /// project, in its order, and a feature whose entry is false is passed
    /// over - not drawn, and not in `unplaced` either, since nothing was asked
    /// of it. The whole project is validated all the same. Empty means every
    /// feature; any other size than the project's is InvalidArgument. A list
    /// of flags and not of indices, so that "none of them" is a list of
    /// falses and can never be mistaken for the empty list that means all.
    pub consider: Vec<bool>,
    /// As LineworkOptions::earlier_lines: the lines an earlier run drew of
    /// these features, each of which is redrawn as the same entity by the
    /// feature that carries its code and string number.
    pub earlier_lines: Vec<EntityId>,
}

/// Why a feature is not drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum UnplacedFeatureReason {
    /// Fewer than two of its points have a position: no line.
    #[default]
    TooFewPoints,
    /// Its positioned points are all in one place: no length to draw.
    Coincident,
    /// only_ruled_lines only, and decided before the two above: no rule more
    /// specific than the bare "*" knows its string name.
    NoRule,
    /// Its rules make it a point, or never say it is a line.
    PointCode,
}

/// The C++ `toString(UnplacedFeatureReason)`.
impl EnumName for UnplacedFeatureReason {
    fn name(self) -> &'static str {
        match self {
            Self::TooFewPoints => "fewer than two of its points have a position",
            Self::Coincident => "its positioned points are all in one place",
            Self::NoRule => "no rule for its code",
            Self::PointCode => "its code is a point code",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UnplacedFeature {
    /// In SurveyProject::features.
    pub index: usize,
    pub name: String,
    pub code: String,
    pub reason: UnplacedFeatureReason,
}

#[derive(Default)]
pub struct SurveyFeatureResult {
    /// ONE undoable command: the layers and the lines, with their styling;
    /// None when no feature makes a line (or every line is an earlier one
    /// that already has these vertices).
    pub command: Option<CommandPtr>,
    /// One LineworkString per feature drawn, in the project's order: `name` is
    /// the feature's name, or its code when it has none; `key` and `number`
    /// split its CODE (the code is what the rules are keyed on and what the
    /// line carries) - under only_ruled_lines, see there for all three;
    /// `points` is EMPTY, because the points are the project's and need not be
    /// in the drawing, and `point_numbers` holds the project's point ids in
    /// the order joined. Also the notes (a point without a position, a close
    /// on two points, a code no rule knows), layers_created and styling.
    /// `considered`, `not_points`, `unplaced` and `points_removed` are about
    /// point entities and stay empty.
    pub report: LineworkReport,
    /// In the project's order.
    pub unplaced: Vec<UnplacedFeature>,
}

impl fmt::Debug for SurveyFeatureResult {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SurveyFeatureResult")
            .field(
                "command",
                &self.command.as_ref().map(|command| command.name()),
            )
            .field("report", &self.report)
            .field("unplaced", &self.unplaced)
            .finish()
    }
}

/// Plans the lines of `project`'s features on `document`, from the project's
/// own coordinates (a SurveyPoint is (easting, northing) in the drawing, as
/// import_survey_project puts it, and its height is kept per vertex, absent
/// staying absent). Straight segments only: a feature carries no curve, so the
/// control codes process_linework reads are not looked for here.
///
/// Fails with the survey's validate_project error for an inconsistent
/// project, as import_survey_project does; with InvalidArgument for a rule
/// model that is not a valid layer name or for a `consider` list of the wrong
/// length; or with NotFound for a layer the drawing lacks when
/// coding.create_layers is off and neither the rule's model nor the points'
/// layer exists (a line put on some other layer would look right and be
/// wrong). Never fails on a feature it cannot draw: that is in `unplaced`.
///
/// The command must be executed on THIS document, as process_linework's.
pub fn draw_survey_features(
    document: &Document,
    project: &SurveyProject,
    options: &SurveyFeatureOptions,
) -> Result<SurveyFeatureResult> {
    draw_survey_features_to(
        CodingSource::of(document),
        document.state_link(),
        project,
        options,
    )
}

/// draw_survey_features over the parts it reads, as process_linework_to.
pub(crate) fn draw_survey_features_to(
    source: CodingSource<'_>,
    link: StateLink,
    project: &SurveyProject,
    options: &SurveyFeatureOptions,
) -> Result<SurveyFeatureResult> {
    // Asked of the survey layer, as import_survey_project asks it, so the two
    // cannot disagree about a valid project. It also guarantees what the loop
    // relies on: every point a feature names is in one of the two lists, and
    // every coordinate is finite.
    validate_project(project)?;
    let consider = &options.consider;
    if !consider.is_empty() && consider.len() != project.features.len() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "the list saying which survey features to draw must have one entry for each feature",
            format!(
                "entries={} features={}",
                consider.len(),
                project.features.len()
            ),
        ));
    }

    let model = source.model;
    let map = source.survey_map;
    let import = &options.import;
    let mut unplaced: Vec<UnplacedFeature> = Vec::new();

    let mut positioned: BTreeMap<&str, &SurveyPoint> = BTreeMap::new();
    for point in &project.points {
        positioned.entry(point.id.as_str()).or_insert(point);
    }

    // An import that wrote no code (an empty import.code_property) still
    // gives the LINES theirs, under the first candidate name - see
    // LineBuilder - or nothing would style them, and nothing would say so.
    // Never asked whether the drawing holds a line already: a feature is
    // drawn by the import that owns it, and a second import of one file
    // draws, and owns, its own.
    let mut builder = LineBuilder::new(
        source,
        link,
        &import.code_property,
        options.coding.create_layers,
        &options.earlier_lines,
        false,
        LineworkReport::default(),
    );
    let ruled = options.only_ruled_lines;
    for (index, feature) in project.features.iter().enumerate() {
        if !consider.is_empty() && !consider[index] {
            continue; // nothing was asked of it: neither drawn nor unplaced
        }
        // Under only_ruled_lines the name is a string number, so the feature is
        // called by its code and number and looked up by the two together;
        // otherwise by whatever the file calls it, and its code is looked up.
        let name = if feature.name.is_empty() {
            feature.code.clone()
        } else if ruled && !feature.code.is_empty() {
            format!("{} {}", feature.code, feature.name)
        } else {
            feature.name.clone()
        };
        let lookup_name = if ruled {
            survey_lookup_name(map, &feature.code, &feature.name)
        } else {
            feature.code.clone()
        };
        let unplace_feature = |unplaced: &mut Vec<UnplacedFeature>,
                               reason: UnplacedFeatureReason| {
            unplaced.push(UnplacedFeature {
                index,
                name: name.clone(),
                code: feature.code.clone(),
                reason,
            });
        };
        if ruled {
            // Before anything is read of its points: a feature the rules do
            // not make a line says so, not that it was too short to be one.
            let as_ruled = ruled_as(map, &lookup_name);
            if as_ruled != RuledAs::Line {
                unplace_feature(
                    &mut unplaced,
                    if as_ruled == RuledAs::NoRule {
                        UnplacedFeatureReason::NoRule
                    } else {
                        UnplacedFeatureReason::PointCode
                    },
                );
                continue;
            }
        }

        // The feature's points as candidates, in the file's order. They carry
        // no control codes: a feature says its closure itself, and shape_of
        // then only joins them straight and checks the closure.
        let mut candidates: Vec<Candidate> = Vec::new();
        let mut sources: Vec<&SurveyPoint> = Vec::new();
        for id in &feature.point_ids {
            let Some(&point) = positioned.get(id.as_str()) else {
                // validate_project put it in unpositioned_points.
                let missing = Candidate {
                    number: id.clone(),
                    ..Candidate::default()
                };
                note(
                    &mut builder.report.notes,
                    &missing,
                    LineworkNoteKind::UnpositionedPoint,
                    name.as_str(),
                );
                continue;
            };
            candidates.push(Candidate {
                number: point.id.clone(),
                // Easting first, as import_survey_project draws a point: a CAD
                // x is an easting.
                at: Point2::new(point.easting, point.northing),
                height: point.elevation,
                code_text: feature.code.clone(),
                ..Candidate::default()
            });
            sources.push(point);
        }
        if candidates.len() < 2 {
            unplace_feature(&mut unplaced, UnplacedFeatureReason::TooFewPoints);
            continue;
        }

        let run = Run {
            points: candidates.iter().collect(),
            closed: feature.closed,
            rectangle: false,
        };
        // Straight only: no tolerance used.
        let shape = shape_of(&run, 1.0, &mut builder.report.notes);
        if !(shape.length() > tolerance::GEOMETRIC) {
            unplace_feature(&mut unplaced, UnplacedFeatureReason::Coincident);
            continue;
        }

        // The rules are keyed on the CODE, and the line carries the code, so
        // the code is what is split and resolved - the name may be anything
        // the file calls the string ("Kerb 1"). Under only_ruled_lines the
        // name is the string number and `lookup_name` is what the two resolve
        // by.
        let split = split_string_name(map, &lookup_name);
        if !split.matched {
            note(
                &mut builder.report.notes,
                &candidates[0],
                LineworkNoteKind::NoRuleForName,
                feature.code.as_str(),
            );
        } else if split.fallback_only {
            note(
                &mut builder.report.notes,
                &candidates[0],
                LineworkNoteKind::FallbackOnlyName,
                feature.code.as_str(),
            );
        }
        // With its points when no rule names a model: the layer the import
        // put the first of them on.
        let points_layer = layer_for_point(sources[0], import);
        if let Err(status) = validate_layer_path(&points_layer) {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "the layer the import options give this feature's points is not a valid layer \
                 name",
                format!("feature={name} layer={points_layer} {}", status.context),
            ));
        }
        let layer = builder.layer_for(&map.lookup(&lookup_name), &feature.code, &points_layer)?;
        if !model.layers.contains(&layer) && !options.coding.create_layers {
            return Err(make_error(
                ErrorCode::NotFound,
                "the drawing has no layer for this feature and layers may not be created",
                format!("feature={name} layer={layer}"),
            ));
        }

        let built = LineworkString {
            number: if ruled && !feature.name.is_empty() {
                feature.name.clone()
            } else {
                split.number
            },
            key: split.key,
            name,
            layer,
            ..LineworkString::default()
        };
        // The string number beside the code, as the feature's points carry
        // it, so apply_survey_codes looks the line up by the same two.
        let string_property = if ruled { SURVEY_STRING_PROPERTY } else { "" };
        builder.emit(
            built,
            &feature.code,
            &shape,
            &run.points,
            &[
                (
                    import.description_property.as_str(),
                    feature.description.as_str(),
                ),
                (string_property, feature.name.as_str()),
            ],
        );
    }

    let (command, report) =
        builder.finish("DRAW_SURVEY_FEATURES", options.coding.clone(), Vec::new());
    Ok(SurveyFeatureResult {
        command,
        report,
        unplaced,
    })
}
