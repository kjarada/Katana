//! UTILITY: the AS 5488 subsurface utility tools (katana_survey::subsurface,
//! docs/subsurface_utilities.md) on the text command line - the
//! interpreter's, so the window, katana_cli, katana_mcp and an AI agent all
//! run the same verbs (include/katana/cad/utilities/utility_verbs.hpp,
//! src/katana_cad/utilities/utility_verbs.cpp).
//!
//! ```text
//! UTILITY REPORT    <schedule.csv> | <scope> [MINCOVER <m>] [SPACING <m>]
//! UTILITY VERIFY    <schedule.csv> | <scope>
//! UTILITY CLEARANCE <schedule.csv> | <scope> [DESIGN] <design.csv> | #<id> [LEVEL <z>] |
//!                   ALIGNMENT <name> [WIDTH <m>] [H <m>] [V <m>] [MARGIN <m>]
//! UTILITY CHECK     <schedule.csv> | <scope> SCHEMA <schema.csv>
//! UTILITY DRAW      <schedule.csv> [SPACING <m>] [MINCOVER <m>] [LAYER <prefix>]
//! UTILITY DRAW      <scope> [TYPE <type>] [METHOD <method>] [H_UNC <m>] [V_UNC <m>]
//!                   [HEIGHTS surface|service|none] [LEVEL_REF top|centre|invert]
//!                   [PATH detected|exposed|assumed] [OWNER <text>] [MATERIAL <text>]
//!                   [DIAMETER_MM <mm>] [STATUS <status>] [FIELDS column=property,...]
//!                   [SPACING <m>] [MINCOVER <m>] [LAYER <prefix>]
//! UTILITY REGRADE   <scope> [SPACING <m>] [MINCOVER <m>]
//! UTILITY SCHEDULE  <out.csv> <scope> [SCHEMA <schema.csv>]
//! ```
//!
//! Words are case-insensitive and a path with blanks is quoted. <scope> is the
//! shared scope and filter words (scope_verbs.rs): a first word that is one,
//! or WHERE, takes the drawing instead of a file. For DRAW it takes the lines,
//! polylines and points a survey or an import left there, which it draws as
//! services (read_geometry_services, utility_data.rs); for every other verb it
//! takes the services DRAW drew, each line whole. REPORT, VERIFY, CLEARANCE
//! and CHECK take a file or the drawing, and on the drawing lead their reply
//! with what the scope took (scope_record); DRAW takes either too, and on the
//! drawing, like REGRADE and SCHEDULE, which take only the drawing, leads with
//! its own record, then the scope's. A scope that takes nothing to act on is
//! answered with the scope's record and says so, by every verb, and is not a
//! failure. REPORT, VERIFY, CLEARANCE, CHECK and SCHEDULE read and report;
//! they never touch the drawing. CHECK is refused when the schedule has errors
//! against the schema, with the whole check in the refusal, so a script stops
//! there and still shows why. DRAW adds the graded schedule, or the geometry's
//! services, to the drawing as ONE undo step (utility_drawing.rs) and leaves
//! the geometry it read as it was, and REGRADE draws what is drawn again from
//! its points as ONE step; each replies with its records, the first of which
//! carries the bounds= a front end frames.
//!
//! A file's bytes become text by the port's rule for text from outside: UTF-8
//! as it is, anything else read as Windows-1252 (the C++ hands the bytes on
//! as they are, which a Rust str cannot hold).

use std::fs::File;
use std::io::Read;

use katana_core::error::make_error;
use katana_core::path_text::path_from_utf8;
use katana_core::text::{equals_ignoring_case, parse_finite_double, parse_integer};
use katana_core::text_encoding::utf8_or_windows1252;
use katana_core::{Error, ErrorCode, Result};
use katana_entity::EntityId;
use katana_geometry::{Box2, Point2};
use katana_survey::subsurface::{
    self as sub, ClearanceRequirement, DeliverySchema, DesignAlignment, FindingSeverity,
    GradingSettings, UtilityCsvDialect, UtilityLine,
};

use super::utility_data::{
    DESIGN_CHORD_TOLERANCE, GeometryHeights, GeometryServiceOptions, UtilityData, UtilityRegrade,
    UtilityRegradeOptions, design_from_alignment, design_from_entity, draw_geometry_services,
    geometry_service_keys, parse_geometry_heights, plan_utility_regrade, read_geometry_services,
    read_utility_data, trimmed_text, utility_data_keys,
};
use super::utility_drawing::{
    DEFAULT_UTILITY_LAYER_PREFIX, UTILITIES_DRAWN_RECORD, UtilityDrawOptions, draw_utilities,
    format_utility_drawing, utility_draw_command,
};
use crate::document::Document;
use crate::scope_verbs::{
    ScopeViewProvider, ScopeWords, is_scope_word, match_scope, parse_scope_words, record_value,
    scope_record,
};

// Each verb's usage names the schedule first, as it did before the drawing
// became a source too, then the scope words that are the other source.
const REPORT_USAGE: &str = "UTILITY REPORT <schedule.csv> [MINCOVER <m>] [SPACING <m>], or \
     UTILITY REPORT <scope> [WHERE key=value ...] [MINCOVER <m>] [SPACING <m>]";
const VERIFY_USAGE: &str =
    "UTILITY VERIFY <schedule.csv>, or UTILITY VERIFY <scope> [WHERE key=value ...]";
const CLEARANCE_USAGE: &str = "UTILITY CLEARANCE <schedule.csv> | <scope> [WHERE key=value ...] \
     DESIGN <design.csv> | #<entity id> [LEVEL <z>] | ALIGNMENT <name> [WIDTH <m>] [H <m>] [V \
     <m>] [MARGIN <m>]";
const CHECK_USAGE: &str = "UTILITY CHECK <schedule.csv> SCHEMA <schema.csv>, or UTILITY CHECK \
     <scope> [WHERE key=value ...] SCHEMA <schema.csv>";
const DRAW_USAGE: &str = "UTILITY DRAW <schedule.csv> [SPACING <m>] [MINCOVER <m>] [LAYER \
     <prefix>], or UTILITY DRAW <scope> [WHERE key=value ...] [TYPE <type>] [METHOD <method>] \
     [H_UNC <m>] [V_UNC <m>] [HEIGHTS surface|service|none] [LEVEL_REF top|centre|invert] [PATH \
     detected|exposed|assumed] [OWNER <text>] [MATERIAL <text>] [DIAMETER_MM <mm>] [STATUS \
     <status>] [FIELDS column=property,...] [SPACING <m>] [MINCOVER <m>] [LAYER <prefix>]";
const REGRADE_USAGE: &str =
    "UTILITY REGRADE <scope> [WHERE key=value ...] [SPACING <m>] [MINCOVER <m>]";
const SCHEDULE_USAGE: &str =
    "UTILITY SCHEDULE <out.csv> <scope> [WHERE key=value ...] [SCHEMA <schema.csv>]";

fn usage(text: &str) -> Error {
    Error::new(ErrorCode::InvalidArgument, format!("usage: {text}"))
}

fn same_word(word: &str, name: &str) -> bool {
    equals_ignoring_case(word.as_bytes(), name.as_bytes())
}

// A path typed on the command line is UTF-8 text (path_from_utf8, which every
// verb that names a file shares). A word of the interpreter is a str, so the
// C++'s other door - bytes that are not UTF-8, opened as the narrow name they
// are - has no Rust twin.
//
// As the C++'s stream, a file that opens and then fails to read (a directory,
// on Linux) is the text read so far.
fn read_file(path: &str) -> Result<String> {
    let Ok(mut file) = File::open(path_from_utf8(path)) else {
        return Err(Error::new(
            ErrorCode::NotFound,
            format!("cannot read {path}"),
        ));
    };
    let mut bytes = Vec::new();
    let _ = file.read_to_end(&mut bytes);
    Ok(utf8_or_windows1252(bytes))
}

// A reader's error with the file it was reading named, so a batch that reads
// three files says which one was wrong.
fn in_file(mut error: Error, path: &str) -> Error {
    error.context = if error.context.is_empty() {
        String::from(path)
    } else {
        format!("{} in {path}", error.context)
    };
    error
}

fn read_schedule(path: &str) -> Result<Vec<UtilityLine>> {
    let text = read_file(path)?;
    sub::parse_utility_csv(&text).map_err(|error| in_file(error, path))
}

fn read_schema(path: &str) -> Result<DeliverySchema> {
    let text = read_file(path)?;
    sub::parse_delivery_schema(&text).map_err(|error| in_file(error, path))
}

// A report as a reply: the interpreter's replies end without a line break,
// and the front end adds one, so the text printed is the report exactly.
fn reply(mut text: String) -> String {
    while text.ends_with('\n') {
        text.pop();
    }
    text
}

// KEYWORD <value> pairs after the other arguments, each keyword at most once.
// A length must be finite and not negative, a size more than nothing; a level
// may be any finite number, below the datum too; a word is taken as written,
// for the verb to read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OptionKind {
    Metres,
    Level,
    Positive,
    Word,
}

#[derive(Clone, Copy, Debug)]
struct OptionSpec {
    name: &'static str,
    kind: OptionKind,
    // What the refusal of a value that is missing, or does not read, says it
    // needs; empty for the kind's own words.
    needs: &'static str,
}

impl OptionSpec {
    const fn metres(name: &'static str) -> Self {
        Self {
            name,
            kind: OptionKind::Metres,
            needs: "",
        }
    }

    const fn of(name: &'static str, kind: OptionKind, needs: &'static str) -> Self {
        Self { name, kind, needs }
    }
}

#[derive(Default)]
struct Options {
    numbers: Vec<(String, f64)>,
    words: Vec<(String, String)>,
}

impl Options {
    fn get(&self, name: &str) -> Option<f64> {
        self.numbers
            .iter()
            .find(|(key, _)| key == name)
            .map(|&(_, value)| value)
    }

    fn word(&self, name: &str) -> Option<&str> {
        self.words
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
}

fn needs_of(spec: &OptionSpec) -> &'static str {
    if !spec.needs.is_empty() {
        return spec.needs;
    }
    match spec.kind {
        OptionKind::Metres => "a number of metres, not negative",
        OptionKind::Level => "a level, in metres",
        OptionKind::Positive => "a number more than 0",
        OptionKind::Word => "a value",
    }
}

fn read_options(args: &[String], first: usize, allowed: &[OptionSpec]) -> Result<Options> {
    let mut options = Options::default();
    let mut i = first;
    while i < args.len() {
        let mut spec = None;
        for candidate in allowed {
            if same_word(&args[i], candidate.name) {
                spec = Some(candidate);
            }
        }
        let Some(spec) = spec else {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                format!("unknown option {}", args[i]),
            ));
        };
        let name = spec.name;
        if options.get(name).is_some() || options.word(name).is_some() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                format!("{name} given twice"),
            ));
        }
        if spec.kind == OptionKind::Word {
            let Some(value) = args.get(i + 1) else {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    format!("{name} needs {}", needs_of(spec)),
                ));
            };
            options.words.push((String::from(name), value.clone()));
            i += 2;
            continue;
        }
        let value = args
            .get(i + 1)
            .and_then(|word| parse_finite_double(word.as_str()));
        let Some(value) = value.filter(|&value| match spec.kind {
            OptionKind::Level => true,
            OptionKind::Positive => value > 0.0,
            _ => value >= 0.0,
        }) else {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                format!("{name} needs {}", needs_of(spec)),
            ));
        };
        options.numbers.push((String::from(name), value));
        i += 2;
    }
    Ok(options)
}

fn is_option_word(word: &str, allowed: &[OptionSpec]) -> bool {
    allowed.iter().any(|spec| same_word(word, spec.name))
}

fn grading(options: &Options) -> GradingSettings {
    let mut settings = GradingSettings::default();
    if let Some(spacing) = options.get("SPACING") {
        settings.maximum_detected_spacing = spacing;
    }
    settings
}

// ---- where the services come from --------------------------------------------------------

// The first word after the verb's own says: a scope word or WHERE is the
// drawing, by the shared scope words (scope_verbs.rs); anything else is the
// path of a schedule.
struct SourceWords {
    scope: Option<ScopeWords>,
    path: String,
}

fn read_source_words(args: &[String], at: &mut usize) -> Result<SourceWords> {
    if is_scope_word(&args[*at]) {
        let scope = parse_scope_words(args, at)?;
        return Ok(SourceWords {
            scope: Some(scope),
            path: String::new(),
        });
    }
    let path = args[*at].clone();
    *at += 1;
    Ok(SourceWords { scope: None, path })
}

struct Source {
    lines: Vec<UtilityLine>,
    // The drawing: what the scope took, as the first record of the reply, and
    // the services it read, each whole.
    record: Option<String>,
    data: Option<UtilityData>,
}

fn load_source(
    document: &Document,
    words: &SourceWords,
    views: ScopeViewProvider<'_>,
) -> Result<Source> {
    let Some(scope) = &words.scope else {
        return Ok(Source {
            lines: read_schedule(&words.path)?,
            record: None,
            data: None,
        });
    };
    let found = match_scope(document, scope, views)?;
    let data = read_utility_data(document.model(), &found.matched)?;
    Ok(Source {
        record: Some(format!(
            "{}{}",
            scope_record(&found),
            utility_data_keys(&data)
        )),
        lines: data.lines(),
        data: Some(data),
    })
}

// A reply on drawing data leads with what the scope took; a scope that took no
// service says so, and is not a failure.
fn with_record(source: &Source, text: String) -> String {
    match &source.record {
        Some(record) => format!("{record}\n{text}"),
        None => text,
    }
}

fn nothing_in(source: &Source, what: &str) -> String {
    let mut text = format!(
        "{}\nno utility lines in the scope: nothing {what}",
        source.record.as_deref().unwrap_or_default()
    );
    // Survey or imported geometry is not a service until it is drawn as one; a
    // person who scoped it is most likely asking for that.
    if source.data.as_ref().is_some_and(|data| data.ignored > 0) {
        text += "\nwhat the scope took carries no utility data; UTILITY DRAW with the same scope \
                 and METHOD <method> draws its lines and polylines as services";
    }
    text
}

fn report(document: &Document, args: &[String], views: ScopeViewProvider<'_>) -> Result<String> {
    if args.len() < 2 {
        return Err(usage(REPORT_USAGE));
    }
    let mut at = 1;
    let words = read_source_words(args, &mut at)?;
    let options = read_options(
        args,
        at,
        &[
            OptionSpec::metres("MINCOVER"),
            OptionSpec::metres("SPACING"),
        ],
    )?;
    let source = load_source(document, &words, views)?;
    if source.record.is_some() && source.lines.is_empty() {
        return Ok(nothing_in(&source, "to report"));
    }
    let text = sub::render_investigation_report(
        &source.lines,
        &grading(&options),
        options.get("MINCOVER"),
    )?;
    Ok(with_record(&source, reply(text)))
}

fn verify(document: &Document, args: &[String], views: ScopeViewProvider<'_>) -> Result<String> {
    if args.len() < 2 {
        return Err(usage(VERIFY_USAGE));
    }
    let mut at = 1;
    let words = read_source_words(args, &mut at)?;
    if at != args.len() {
        return Err(usage(VERIFY_USAGE));
    }
    let source = load_source(document, &words, views)?;
    if source.record.is_some() && source.lines.is_empty() {
        return Ok(nothing_in(&source, "to verify"));
    }
    let settings = GradingSettings::default();
    let verified = sub::verify_detections(&source.lines, &settings);
    Ok(with_record(
        &source,
        reply(sub::render_verification_report(&verified, &settings)),
    ))
}

// The proposed works CLEARANCE measures against: a design file, an entity
// drawn as the centre line, or a document alignment.
#[derive(Default)]
struct DesignWords {
    path: String,
    entity: Option<EntityId>,
    alignment: Option<String>,
}

const CLEARANCE_OPTIONS: [OptionSpec; 5] = [
    OptionSpec::metres("WIDTH"),
    OptionSpec::metres("H"),
    OptionSpec::metres("V"),
    OptionSpec::metres("MARGIN"),
    OptionSpec::of("LEVEL", OptionKind::Level, ""),
];

fn read_design_words(args: &[String], at: &mut usize) -> Result<DesignWords> {
    // DESIGN may be left out: the design file second, as it always was.
    if *at < args.len() && same_word(&args[*at], "DESIGN") {
        *at += 1;
    }
    if *at >= args.len() || is_option_word(&args[*at], &CLEARANCE_OPTIONS) {
        return Err(usage(CLEARANCE_USAGE));
    }
    let word = &args[*at];
    *at += 1;
    let mut design = DesignWords::default();
    if word.len() > 1 && word.starts_with('#') {
        let Some(id) = parse_integer(&word[1..])
            .filter(|&id| id >= 1)
            .and_then(|id| EntityId::try_from(id).ok())
        else {
            return Err(make_error(
                ErrorCode::ParseFailure,
                "a design entity is #<entity id>",
                word.as_str(),
            ));
        };
        design.entity = Some(id);
    } else if same_word(word, "ALIGNMENT") {
        let Some(name) = args.get(*at) else {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "ALIGNMENT needs the alignment's name; ALIGN LIST lists them",
            ));
        };
        design.alignment = Some(name.clone());
        *at += 1;
    } else {
        design.path.clone_from(word);
    }
    Ok(design)
}

fn load_design(
    document: &Document,
    words: &DesignWords,
    level: Option<f64>,
) -> Result<DesignAlignment> {
    if level.is_some() && words.entity.is_none() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "LEVEL is the level of a design drawn as an entity (#<entity id>); a design file and \
             an alignment carry their own",
        ));
    }
    if let Some(id) = words.entity {
        let Some(entity) = document.model().entities.find(id) else {
            return Err(Error::new(
                ErrorCode::NotFound,
                format!("no entity #{id} in the drawing"),
            ));
        };
        return design_from_entity(entity, level);
    }
    if let Some(name) = &words.alignment {
        let Some(alignment) = document.model().alignments.find(name) else {
            return Err(Error::new(
                ErrorCode::NotFound,
                format!("no alignment named {name}; ALIGN LIST lists them"),
            ));
        };
        return design_from_alignment(alignment, DESIGN_CHORD_TOLERANCE);
    }
    let text = read_file(&words.path)?;
    sub::parse_design_csv(&text, words.path.clone()).map_err(|error| in_file(error, &words.path))
}

fn clearance(document: &Document, args: &[String], views: ScopeViewProvider<'_>) -> Result<String> {
    if args.len() < 3 {
        return Err(usage(CLEARANCE_USAGE));
    }
    let mut at = 1;
    let words = read_source_words(args, &mut at)?;
    let design_words = read_design_words(args, &mut at)?;
    let options = read_options(args, at, &CLEARANCE_OPTIONS)?;
    let source = load_source(document, &words, views)?;
    let mut design = load_design(document, &design_words, options.get("LEVEL"))?;
    if source.record.is_some() && source.lines.is_empty() {
        return Ok(nothing_in(&source, "to clear"));
    }
    design.half_width = options.get("WIDTH").unwrap_or(0.0) / 2.0;
    let defaults = ClearanceRequirement::default();
    let requirement = ClearanceRequirement {
        horizontal: options.get("H").unwrap_or(defaults.horizontal),
        vertical: options.get("V").unwrap_or(defaults.vertical),
        unverified_margin: options.get("MARGIN").unwrap_or(defaults.unverified_margin),
    };

    let results = sub::check_clearance(
        &design,
        &source.lines,
        &requirement,
        &GradingSettings::default(),
    )?;
    Ok(with_record(
        &source,
        reply(sub::render_clearance_report(
            &design,
            &results,
            &requirement,
        )),
    ))
}

// UTILITY CHECK <schedule.csv | scope> SCHEMA <schema.csv>. A schedule with
// errors against the schema is REFUSED, so that a script gates a delivery on
// it - and the refusal carries the whole check, since a script that stops
// needs to say why. The drawing is checked as the schedule UTILITY SCHEDULE
// would write from it in the schema's own words (utility_csv_dialect): what
// the drawing would deliver.
fn check(document: &Document, args: &[String], views: ScopeViewProvider<'_>) -> Result<String> {
    if args.len() < 2 {
        return Err(usage(CHECK_USAGE));
    }
    let mut at = 1;
    let words = read_source_words(args, &mut at)?;
    if at + 2 != args.len() || !same_word(&args[at], "SCHEMA") {
        return Err(usage(CHECK_USAGE));
    }
    let schema = read_schema(&args[at + 1])?;
    let (schedule_text, drawn) = if words.scope.is_some() {
        let source = load_source(document, &words, views)?;
        if source.lines.is_empty() {
            return Ok(nothing_in(&source, "to check"));
        }
        let text = sub::write_utility_csv(&source.lines, &sub::utility_csv_dialect(&schema))?;
        (text, Some(source))
    } else {
        (read_file(&words.path)?, None)
    };
    let result = match sub::check_delivery(&schedule_text, &schema) {
        Ok(result) => result,
        Err(error) if words.scope.is_some() => return Err(error),
        Err(error) => return Err(in_file(error, &words.path)),
    };
    let mut text = reply(sub::render_schema_check(&result, &schema));
    if let Some(drawn) = &drawn {
        text = with_record(drawn, text);
    }
    let errors = result.count(FindingSeverity::Error);
    if errors != 0 {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!(
                "the schedule does not meet the schema: {errors}{}\n{text}",
                if errors == 1 { " error" } else { " errors" }
            ),
        ));
    }
    Ok(text)
}

// What a draw from the drawing is told of the geometry it reads, beside
// SPACING, MINCOVER and LAYER: each the schedule's column of that name, with
// the value every row of every line in scope would have where the line or the
// point does not say it itself (read_geometry_services). HEIGHTS says what the
// geometry's heights are the heights of.
const GEOMETRY_OPTIONS: [OptionSpec; 12] = [
    OptionSpec::of(
        "TYPE",
        OptionKind::Word,
        "a utility type: water, electricity, gas ...",
    ),
    OptionSpec::of(
        "METHOD",
        OptionKind::Word,
        "a location method: EML, GPR, pothole, surface, records ...",
    ),
    OptionSpec::of(
        "H_UNC",
        OptionKind::Metres,
        "a horizontal uncertainty, in metres, not negative",
    ),
    OptionSpec::of(
        "V_UNC",
        OptionKind::Metres,
        "a vertical uncertainty, in metres, not negative",
    ),
    OptionSpec::of("HEIGHTS", OptionKind::Word, "surface, service or none"),
    OptionSpec::of("LEVEL_REF", OptionKind::Word, "top, centre or invert"),
    OptionSpec::of("PATH", OptionKind::Word, "detected, exposed or assumed"),
    OptionSpec::of("OWNER", OptionKind::Word, "the owner's name"),
    OptionSpec::of("MATERIAL", OptionKind::Word, "a material"),
    OptionSpec::of(
        "DIAMETER_MM",
        OptionKind::Positive,
        "a diameter in millimetres, more than 0",
    ),
    OptionSpec::of(
        "STATUS",
        OptionKind::Word,
        "a status: in service, disused, abandoned or proposed",
    ),
    OptionSpec::of(
        "FIELDS",
        OptionKind::Word,
        "column=property pairs, comma-separated: type=ASSET_TYPE,method=LOC_METHOD",
    ),
];

const DRAW_OPTIONS: [OptionSpec; 3] = [
    OptionSpec::metres("SPACING"),
    OptionSpec::metres("MINCOVER"),
    OptionSpec::of("LAYER", OptionKind::Word, "a layer name"),
];

// A word of the options that must read as one of `parse`'s values.
fn read_word<T>(
    options: &Options,
    name: &str,
    parse: impl Fn(&str) -> Option<T>,
    what: &str,
) -> Result<Option<T>> {
    let Some(word) = options.word(name) else {
        return Ok(None);
    };
    match parse(word) {
        Some(value) => Ok(Some(value)),
        None => Err(Error::new(
            ErrorCode::InvalidArgument,
            format!("{name} {word} is not {what}"),
        )),
    }
}

fn geometry_service_options(options: &Options) -> Result<GeometryServiceOptions> {
    let r#type = read_word(
        options,
        "TYPE",
        sub::parse_utility_type,
        "a utility type (water, electricity, telecommunications, gas, recycled water, fire \
         service, sewer, stormwater, fuel, ITS, other)",
    )?;
    let method = read_word(
        options,
        "METHOD",
        sub::parse_location_method,
        "a location method (records, anecdotal, surface, EML, GPR, geophysical, pothole, trench)",
    )?;
    let level_reference = read_word(
        options,
        "LEVEL_REF",
        sub::parse_level_reference,
        "a level reference (top, centre, invert)",
    )?;
    let path = read_word(
        options,
        "PATH",
        sub::parse_path_evidence,
        "detected, exposed or assumed",
    )?;
    let status = read_word(
        options,
        "STATUS",
        sub::parse_utility_status,
        "a status (in service, disused, abandoned, proposed)",
    )?;
    let heights = read_word(
        options,
        "HEIGHTS",
        parse_geometry_heights,
        "surface, service or none: what the heights are the heights of",
    )?;
    // FIELDS column=property,...: which column each is, read_geometry_services
    // says, by the schedule reader's own names.
    let mut fields = Vec::new();
    if let Some(text) = options.word("FIELDS") {
        let mut rest = text;
        while !rest.is_empty() {
            let comma = rest.find(',');
            let pair = trimmed_text(&rest[..comma.unwrap_or(rest.len())]);
            let Some(equals) = pair.find('=') else {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    format!(
                        "FIELDS needs column=property pairs, comma-separated; \"{pair}\" is not \
                         one"
                    ),
                ));
            };
            fields.push((
                String::from(trimmed_text(&pair[..equals])),
                String::from(trimmed_text(&pair[equals + 1..])),
            ));
            rest = match comma {
                Some(comma) => &rest[comma + 1..],
                None => "",
            };
        }
    }
    Ok(GeometryServiceOptions {
        r#type,
        method,
        horizontal_uncertainty: options.get("H_UNC"),
        vertical_uncertainty: options.get("V_UNC"),
        level_reference,
        path,
        owner: String::from(options.word("OWNER").unwrap_or_default()),
        material: String::from(options.word("MATERIAL").unwrap_or_default()),
        diameter: options
            .get("DIAMETER_MM")
            .map(|millimetres| millimetres / 1000.0),
        status,
        heights: heights.unwrap_or(GeometryHeights::Surface),
        fields,
    })
}

fn drawing_options(options: &Options) -> UtilityDrawOptions {
    UtilityDrawOptions {
        grading: grading(options),
        layer_prefix: String::from(
            options
                .word("LAYER")
                .unwrap_or(DEFAULT_UTILITY_LAYER_PREFIX),
        ),
        minimum_cover: options.get("MINCOVER"),
    }
}

// The draw's or the regrade's records with `record` - what the scope took -
// after their first: the first is what a front end frames by.
fn after_first(records: &str, record: &str) -> String {
    match records.find('\n') {
        Some(end) => format!("{}\n{record}{}", &records[..end], &records[end..]),
        None => format!("{records}\n{record}"),
    }
}

// UTILITY DRAW <scope> ...: the lines and polylines a survey or an import left
// in the drawing, drawn as the services they are, as one undo step. The reply
// is a draw's, with what the scope took after its first record, as a
// regrade's is.
fn draw_from_geometry(
    document: &mut Document,
    args: &[String],
    views: ScopeViewProvider<'_>,
) -> Result<String> {
    let mut at = 1;
    let words = read_source_words(args, &mut at)?;
    let allowed: Vec<OptionSpec> = DRAW_OPTIONS
        .iter()
        .chain(GEOMETRY_OPTIONS.iter())
        .copied()
        .collect();
    let options = read_options(args, at, &allowed)?;
    let geometry = geometry_service_options(&options)?;
    // draw() sends a line here only when its first word is a scope word.
    let Some(scope) = &words.scope else {
        return Err(usage(DRAW_USAGE));
    };
    let found = match_scope(document, scope, views)?;
    let services = read_geometry_services(document.model(), &found.matched, &geometry)?;
    let record = format!(
        "{}{}",
        scope_record(&found),
        geometry_service_keys(&services)
    );
    if services.lines.is_empty() {
        return Ok(format!(
            "{record}\nno lines or open polylines in the scope that are not drawn already: \
             nothing drawn"
        ));
    }
    let drawing = draw_geometry_services(&services, &drawing_options(&options))?;
    let command = utility_draw_command(document.model(), &drawing);
    document.execute(command)?;
    Ok(after_first(
        &format_utility_drawing(&drawing, UTILITIES_DRAWN_RECORD),
        &record,
    ))
}

// UTILITY DRAW <schedule.csv> [SPACING <m>] [MINCOVER <m>] [LAYER <prefix>]:
// the graded schedule into the drawing as one undo step (utility_drawing.rs);
// or UTILITY DRAW <scope> ..., the geometry in the drawing (draw_from_geometry).
fn draw(document: &mut Document, args: &[String], views: ScopeViewProvider<'_>) -> Result<String> {
    if args.len() < 2 {
        return Err(usage(DRAW_USAGE));
    }
    if is_scope_word(&args[1]) {
        return draw_from_geometry(document, args, views);
    }
    // A schedule says how each vertex was located and what each service is.
    for word in args.iter().skip(2).step_by(2) {
        if let Some(spec) = GEOMETRY_OPTIONS
            .iter()
            .find(|spec| same_word(word, spec.name))
        {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                format!(
                    "{} is for a draw of geometry in the drawing (UTILITY DRAW <scope> ...); a \
                     schedule gives its own in its columns",
                    spec.name
                ),
            ));
        }
    }
    let options = read_options(args, 2, &DRAW_OPTIONS)?;
    let lines = read_schedule(&args[1])?;
    let drawing = draw_utilities(&lines, &drawing_options(&options))?;
    let command = utility_draw_command(document.model(), &drawing);
    document.execute(command)?;
    Ok(format_utility_drawing(&drawing, UTILITIES_DRAWN_RECORD))
}

// UTILITY REGRADE <scope> [SPACING <m>] [MINCOVER <m>]: the lines in scope
// graded again from their points, their runs drawn again and their points'
// graded properties replaced, as ONE undo step (utility_data.rs).
fn regrade(
    document: &mut Document,
    args: &[String],
    views: ScopeViewProvider<'_>,
) -> Result<String> {
    if args.len() < 2 || !is_scope_word(&args[1]) {
        return Err(usage(REGRADE_USAGE));
    }
    let mut at = 1;
    let words = read_source_words(args, &mut at)?;
    let options = read_options(
        args,
        at,
        &[
            OptionSpec::metres("SPACING"),
            OptionSpec::metres("MINCOVER"),
        ],
    )?;
    let source = load_source(document, &words, views)?;
    if source.lines.is_empty() {
        return Ok(nothing_in(&source, "to regrade"));
    }
    let Some(data) = &source.data else {
        return Err(usage(REGRADE_USAGE));
    };
    // SPACING and MINCOVER left out are each line's own, as it was drawn.
    let regrade_options = UtilityRegradeOptions {
        spacing: options.get("SPACING"),
        minimum_cover: options.get("MINCOVER"),
        ..UtilityRegradeOptions::default()
    };
    let UtilityRegrade {
        drawing,
        changed,
        command,
    } = plan_utility_regrade(document.model(), data, &regrade_options)?;
    if let Some(command) = command {
        document.execute(command)?;
    }
    // The drawing's records, the regrade's first so a front end frames it as
    // it frames a draw, then what the scope took.
    let records =
        format_utility_drawing(&drawing, &format!("utilities regraded changed={changed}"));
    Ok(after_first(
        &records,
        source.record.as_deref().unwrap_or_default(),
    ))
}

// UTILITY SCHEDULE <out.csv> <scope> [SCHEMA <schema.csv>]: the lines in scope
// written as a schedule the reader takes back - a drawing edited in CAD made a
// deliverable - in the schema's words when one is named.
fn schedule(document: &Document, args: &[String], views: ScopeViewProvider<'_>) -> Result<String> {
    if args.len() < 3 || !is_scope_word(&args[2]) {
        return Err(usage(SCHEDULE_USAGE));
    }
    let path = &args[1];
    let mut at = 2;
    let words = read_source_words(args, &mut at)?;
    let mut schema = None;
    if at < args.len() {
        if at + 2 != args.len() || !same_word(&args[at], "SCHEMA") {
            return Err(usage(SCHEDULE_USAGE));
        }
        schema = Some(read_schema(&args[at + 1])?);
    }
    let source = load_source(document, &words, views)?;
    if source.lines.is_empty() {
        return Ok(nothing_in(&source, "written"));
    }
    let dialect = schema
        .as_ref()
        .map_or_else(UtilityCsvDialect::default, sub::utility_csv_dialect);
    let text = sub::write_utility_csv(&source.lines, &dialect)?;
    if std::fs::write(path_from_utf8(path), text.as_bytes()).is_err() {
        return Err(Error::new(
            ErrorCode::FileExportFailure,
            format!("cannot write {path}"),
        ));
    }
    let vertices: usize = source.lines.iter().map(|line| line.vertices.len()).sum();
    Ok(format!(
        "utilities scheduled path={} lines={} vertices={vertices}\n{}",
        record_value(path),
        source.lines.len(),
        source.record.as_deref().unwrap_or_default()
    ))
}

/// True for UTILITY, in any case.
pub fn is_utility_verb(verb: &str) -> bool {
    same_word(verb, "UTILITY")
}

/// Runs one UTILITY line: `tokens` is the whole line split into words with the
/// quotes removed (as the interpreter's tokenize splits it), the verb first.
/// `views` answers the scope word VIEW (the window's; none headless, the C++
/// default). Returns the reply - a report's text, or DRAW's and REGRADE's
/// records - or an error saying what was refused and why. A refused DRAW or
/// REGRADE changes nothing.
pub fn run_utility_verb(
    document: &mut Document,
    tokens: &[String],
    views: ScopeViewProvider<'_>,
) -> Result<String> {
    if !tokens
        .first()
        .is_some_and(|verb| is_utility_verb(verb.as_str()))
    {
        return Err(make_error(
            ErrorCode::ParseFailure,
            "not a UTILITY command; type HELP UTILITY",
            tokens.first().cloned().unwrap_or_default(),
        ));
    }
    // The action is the first word after the verb; the words from there are
    // as the verb's own functions number them, the action at 0.
    let args = &tokens[1..];
    let action = args
        .first()
        .map(|word| word.to_ascii_lowercase())
        .unwrap_or_default();
    match action.as_str() {
        "report" => report(document, args, views),
        "verify" => verify(document, args, views),
        "clearance" => clearance(document, args, views),
        "check" => check(document, args, views),
        "draw" => draw(document, args, views),
        "regrade" => regrade(document, args, views),
        "schedule" => schedule(document, args, views),
        _ => Err(usage(
            "UTILITY REPORT | VERIFY | CLEARANCE | CHECK | DRAW | REGRADE | SCHEDULE ... (HELP \
             UTILITY)",
        )),
    }
}

/// The bounds= box of a UTILITY DRAW or REGRADE reply's first record, for a
/// front end to frame what arrived: survey data in a real coordinate system
/// usually lands far from the view. The window's command line uses it for
/// typed lines and the utilities dialog alike, so there is one reading of the
/// record. None for any other reply, or a box that is not four finite
/// numbers, min before max; a CR LF line end reads as LF.
pub fn draw_reply_bounds(reply: &str) -> Option<Box2> {
    const KEY: &str = " bounds=";
    let mut first = &reply[..reply.find('\n').unwrap_or(reply.len())];
    // A reply read back from a file or a pipe on Windows ends its lines in CR
    // LF; the record is the same.
    if let Some(without) = first.strip_suffix('\r') {
        first = without;
    }
    let at = first.find(KEY);
    if !(first.starts_with("utilities drawn ") || first.starts_with("utilities regraded ")) {
        return None;
    }
    let mut rest = &first[at? + KEY.len()..];
    rest = &rest[..rest.find(' ').unwrap_or(rest.len())];
    let mut numbers = Vec::new();
    while numbers.len() < 5 {
        let comma = rest.find(',');
        numbers.push(parse_finite_double(&rest[..comma.unwrap_or(rest.len())])?);
        let Some(comma) = comma else {
            break;
        };
        rest = &rest[comma + 1..];
    }
    let [x0, y0, x1, y1] = numbers[..] else {
        return None;
    };
    if x0 > x1 || y0 > y1 {
        return None;
    }
    Some(Box2::new(Point2::new(x0, y0), Point2::new(x1, y1)))
}

/// Every UTILITY verb and option, for HELP UTILITY.
pub fn utility_verb_help() -> String {
    String::from(
        r#"AS 5488 subsurface utilities (docs/subsurface_utilities.md). A schedule is a CSV of
located vertices, one row each; a path with blanks is quoted. Lengths are metres.

Every verb acts on a schedule file OR on what is drawn, but REGRADE and SCHEDULE, which
act on what is drawn only. A first word that is a scope word or WHERE takes the drawing,
by the scope and filter MODIFY takes too (HELP: "Scope") - SELECTION | DRAWING |
VIEW [id] [EXTENTS] | AREA x0,y0,x1,y1 | LAYERS a,b [ONLY], then [WHERE key=value ...];
any other word is a file's path. DRAW takes the lines, polylines and points a survey or
an import left there and draws them as services; every other verb takes the services
DRAW drew, and a scope that takes part of one takes all of it. The scope's record says
what it took: "scope=... matched= lines= completed= ignored=" (completed: lines read
whole from beyond the scope; ignored: entities with no utility data). REPORT, VERIFY,
CLEARANCE and CHECK lead their reply with it; DRAW, REGRADE and SCHEDULE lead with their
own record, then it.

UTILITY REPORT <schedule.csv> | <scope> [MINCOVER <m>] [SPACING <m>]
          grade located services by AS 5488 quality level: vertices, segments, length at
          each level, depth of cover (flagged below MINCOVER), findings. SPACING is the
          longest detected segment that keeps QL-B (default 10)
UTILITY VERIFY <schedule.csv> | <scope>
          QL-B detections against the QL-A exposures that name them in the verifies column
UTILITY CLEARANCE <schedule.csv> | <scope> [DESIGN] <design.csv> | #<id> [LEVEL <z>] |
          ALIGNMENT <name> [WIDTH <m>] [H <m>] [V <m>] [MARGIN <m>]
          clearance of proposed works from each service, widened by its quality level's
          tolerance. The works: a design CSV; a line or polyline drawn (#id), at LEVEL or
          its own heights; or an alignment, at its design profile's levels. WIDTH of the
          works, H and V the clearances required, MARGIN how near an unmeasured (QL-C, QL-D)
          service is unconfirmed
UTILITY CHECK <schedule.csv> | <scope> SCHEMA <schema.csv>
          the schedule against a client's delivery schema: mandatory attributes and their
          value lists (tools/utility_schema_domains.py makes the schema file from a TfNSW
          Utility Schema workbook); the drawing is checked as the schedule it would write
          in the schema's words. Refused, with the whole check, when there are errors
UTILITY DRAW <schedule.csv> [SPACING <m>] [MINCOVER <m>] [LAYER <prefix>]
          the graded services into the drawing as ONE undo step: a polyline per run at one
          quality level on <prefix>/<type>/QL-A..QL-D (linetype by level, colour by type),
          a point per vertex on <prefix>/<type>/points carrying its whole schedule row as
          utility.* properties. LAYER defaults to "utilities"; MINCOVER marks each point's
          cover against it.
          Reply: "utilities drawn lines= vertices= segments= entities= layers= bounds=x0,y0,x1,y1"
          then "line id= type= length= ql_a= ql_b= ql_c= ql_d=" for each service
UTILITY DRAW <scope> [TYPE <type>] [METHOD <method>] [H_UNC <m>] [V_UNC <m>]
          [HEIGHTS surface|service|none] [LEVEL_REF top|centre|invert]
          [PATH detected|exposed|assumed] [OWNER <text>] [MATERIAL <text>]
          [DIAMETER_MM <mm>] [STATUS <status>] [FIELDS column=property,...]
          [SPACING <m>] [MINCOVER <m>] [LAYER <prefix>]
          the surveyed or imported services in the drawing (a coded survey's strings, a .12da,
          DXF, shapefile or IFC file's lines) drawn as a schedule is: each line or open
          polyline in scope a service, named by its code, its vertices the located ones; a
          point in scope on a vertex gives it its point number. What the geometry cannot say
          comes from utility.* properties on the line or the point (utility.method,
          utility.h_unc, utility.level, utility.depth, utility.type ..., as MODIFY ... SET
          PROP= puts them), else from these options: each is the schedule's column of that
          name. HEIGHTS says what the heights are: the surface over the service (default),
          the service itself on LEVEL_REF, or none; utility.heights on a point or a line
          says it for that one. FIELDS reads an import's own attributes as the schedule's
          columns (a shapefile's ASSET_TYPE as type: FIELDS type=ASSET_TYPE,line=ASSET_ID).
          A method is needed at every vertex. Draws only what is not drawn already; the
          sources are left as they are.
          Reply: the draw's first record, then the scope's
          "scope=... matched= lines= points= loose= drawn= ignored=" (points: gave a vertex;
          loose: on no vertex; drawn: drawn already; ignored: cannot be a service), then
          the lines
UTILITY REGRADE <scope> [SPACING <m>] [MINCOVER <m>]
          grade the lines in scope again from their points as they are now (moved, levels
          or methods edited) and draw their runs again, as ONE undo step; nothing changed,
          no step. SPACING and MINCOVER left out are each line's own, as it was drawn.
          Reply: "utilities regraded changed= lines= ... bounds=", the scope, lines
UTILITY SCHEDULE <out.csv> <scope> [SCHEMA <schema.csv>]
          write the lines in scope as a schedule this reads back as they are: a drawing
          edited in CAD made a deliverable, in the schema's words when SCHEMA is given.
          Reply: "utilities scheduled path= lines= vertices=", the scope"#,
    )
}
