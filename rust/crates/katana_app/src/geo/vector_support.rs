//! What the curated vector verbs share (docs/geoprocessing.md, "V1" to
//! "V5"), the port of src/katana_app/geo/vector_support.hpp/.cpp (the C++'s
//! namespace `geo::vector`): GIS BUFFER, DISSOLVE, OVERLAY, HULL, CLIP, CHECK,
//! REPAIR, COVERAGE and SQL are thin modules over the bindings (bindings.rs),
//! and these are the pieces each of them would otherwise write again: reading
//! a line's own words apart from its scope, running one vector algorithm on a
//! feature set, measuring what came back, and applying a result as ONE undo
//! step.
//!
//! Their own words are read the way MODIFY reads SET and PREVIEW: taken out of
//! the line wherever they stand, and the rest handed to the ONE scope parser
//! (cad's parse_scope_words). So options may follow a WHERE filter - "GIS
//! BUFFER DRAWING WHERE TYPE=polyline distance=5" - and the filter still ends
//! where its conditions do. An option key is never a WHERE key: that is why a
//! result's layer is TO LAYER <path> (the one target parser's word) and not a
//! layer= option, which WHERE's LAYER= would silently take for a filter.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::AtomicBool;

use katana_cad::scope_verbs::{ScopeWords, is_where_key, parse_scope_words, parse_where_condition};
use katana_cad::{Document, ModifyFilter};
use katana_commands::{CommandPtr, Transaction, create_entities, create_layer, delete_entities};
use katana_core::error::make_error;
use katana_core::text::format_exact_real;
use katana_core::{Error, ErrorCode, Result, Status};
use katana_entity::entity::{INVALID_ENTITY_ID, property_to_string, set_heights};
use katana_entity::entity_geometry::bounding_box;
use katana_entity::{Entity, EntityId, Geometry, Layer, Model, PropertyValue};
use katana_geometry::{Box2, Point2, Polyline2, Segment2};
use katana_gis::processing::{
    self as gp, ArgValue, DatasetValue, Feature, FeatureSet, FeatureTable, FieldDef, FieldType,
    FieldValue, OutputTo, RunRequest,
};
use katana_gis::{GeoPoint, GeometryKind, VectorGeometry};
use katana_interop::geo::drawing_dataset::result_command;
use katana_interop::geo::{DrawingDatasetOptions, ResultMode, ResultOptions};
use katana_math::cpp;
use katana_math::tolerance::GEOMETRIC;

use super::bindings::{
    BoundDrawing, Target, TargetKind, Tokens, bind_drawing, parse_target, project_crs,
};
use super::geo_verbs::{Context, Prepared, Progress, trimmed_text};
use super::replies::{fixed3, value};
use crate::import_records::path_from_text;

fn text_of(value: &FieldValue) -> String {
    match value {
        FieldValue::Null => String::new(),
        FieldValue::Boolean(flag) => String::from(if *flag { "true" } else { "false" }),
        FieldValue::Integer64(whole) => format!("{whole}"),
        FieldValue::Real(real) => format_exact_real(*real),
        FieldValue::Text(text) => text.clone(),
    }
}

fn empty_geometry(geometry: &VectorGeometry) -> bool {
    geometry.parts.first().is_none_or(Vec::is_empty)
}

// ---- a line's own words ---------------------------------------------------------------------

/// What a verb takes besides its scope.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WordRules {
    /// Keys of key=value options, lower case: "distance", "side" ...
    pub options: Vec<String>,
    /// Bare words, upper case: "PREVIEW", "REPLACE" ...
    pub flags: Vec<String>,
    /// Whether TO <target> is taken.
    pub target: bool,
    /// The verb's usage, for the refusal of a word it does not know.
    pub usage: String,
    /// The words that begin the target: TO, and AS for GIS SQL's AS REPORT.
    pub target_words: Vec<String>,
}

impl Default for WordRules {
    fn default() -> Self {
        Self {
            options: Vec::new(),
            flags: Vec::new(),
            target: false,
            usage: String::new(),
            target_words: vec![String::from("TO")],
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct VerbWords {
    pub scope: ScopeWords,
    /// Whether any scope or filter word was given: none reads as SELECTION,
    /// which a second clause (OVERLAY's WITH) must tell from no scope at all.
    pub scope_given: bool,
    /// By lower-case key, the value as typed.
    pub options: BTreeMap<String, String>,
    /// Upper case.
    pub flags: BTreeSet<String>,
    pub target: Option<Target>,
}

impl VerbWords {
    pub fn has(&self, flag: &str) -> bool {
        self.flags.contains(flag)
    }

    pub fn option(&self, key: &str) -> Option<&String> {
        self.options.get(key)
    }
}

/// Reads words [begin, end) of `tokens`: the options, flags and TO clause
/// `rules` names, wherever they stand, and the rest as ONE scope clause. An
/// option whose key is a WHERE key (cad's is_where_key: layer=) is the option
/// before WHERE and a condition after it.
///
/// InvalidArgument naming the word for an option or flag given twice, an
/// option with no value, a TO where none is taken or given twice; the scope
/// parser's refusal for its own words; and InvalidArgument with the usage for
/// a word that is none of these. A word after LAYERS is always the layer
/// list, so a layer may be called "preview".
pub fn read_verb_words(
    tokens: &Tokens,
    begin: usize,
    end: usize,
    rules: &WordRules,
) -> Result<VerbWords> {
    // The clause alone, so a TO near its end cannot read past it (OVERLAY's
    // subject ends at WITH).
    let stop = end.min(tokens.size());
    let begin = begin.min(stop);
    let clause = Tokens {
        words: tokens.words[begin..stop].to_vec(),
        quoted: tokens.quoted[begin..stop].to_vec(),
    };
    let mut scope: Vec<String> = Vec::new();
    let mut in_where = false;
    let mut first_where: Option<usize> = None;
    let mut out = VerbWords::default();
    let mut i = 0;
    while i < clause.size() {
        let word = clause[i].clone();
        if let Some(equals) = word.find('=')
            && equals > 0
        {
            let key = word[..equals].to_ascii_lowercase();
            // After WHERE, a WHERE key is the filter's: CONTOUR's layer= is
            // the output layer before WHERE and a LAYER= condition after it,
            // which is what the scope widget writes there.
            if rules.options.contains(&key) && !(in_where && is_where_key(&key)) {
                if out.options.contains_key(&key) {
                    return Err(make_error(
                        ErrorCode::InvalidArgument,
                        format!("{key}= is given twice"),
                        word,
                    ));
                }
                out.options.insert(key, String::from(&word[equals + 1..]));
                i += 1;
                continue;
            }
        }
        let folded = if clause.quoted[i] {
            String::new()
        } else {
            word.to_ascii_uppercase()
        };
        if !folded.is_empty() && rules.flags.contains(&folded) {
            if !out.flags.insert(folded.clone()) {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    format!("{folded} is given twice"),
                    word,
                ));
            }
            i += 1;
            continue;
        }
        if !folded.is_empty() && rules.target_words.contains(&folded) {
            if !rules.target {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    format!("{folded} is not a word of this verb: {}", rules.usage),
                    word,
                ));
            }
            if out.target.is_some() {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    format!("one {folded} per line"),
                    word,
                ));
            }
            let mut at = i + 1;
            out.target = Some(parse_target(&clause, &mut at)?);
            i = at;
            continue;
        }
        if folded == "WHERE" {
            first_where.get_or_insert(scope.len());
            in_where = true;
        }
        scope.push(word);
        if !in_where && (folded == "LAYERS" || folded == "LAYER") && i + 1 < clause.size() {
            i += 1;
            scope.push(clause[i].clone());
        }
        i += 1;
    }
    let mut at = 0;
    let parsed = parse_scope_words(&scope, &mut at)?;
    if at < scope.len() {
        // In the filter, a word that is no condition is named as one.
        if let Some(first) = first_where
            && at > first
        {
            let mut unused = ModifyFilter::default();
            parse_where_condition(&scope[at], &mut unused)?;
        }
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!("not a word of this verb: {}", rules.usage),
            scope[at].clone(),
        ));
    }
    out.scope = parsed;
    out.scope_given = !scope.is_empty();
    Ok(out)
}

/// The option `key` as a finite number; None when it was not given.
/// InvalidArgument naming it when it was and does not read.
pub fn number_option(words: &VerbWords, key: &str) -> Result<Option<f64>> {
    let Some(text) = words.option(key) else {
        return Ok(None);
    };
    match katana_core::text::parse_finite_double(text) {
        Some(number) => Ok(Some(number)),
        None => Err(make_error(
            ErrorCode::InvalidArgument,
            format!("{key}= is a number"),
            format!("{key}={text}"),
        )),
    }
}

/// The option `key` as one of `choices` (any case, answered in lower case);
/// `fallback` when it was not given. InvalidArgument listing the choices.
pub fn choice_option(
    words: &VerbWords,
    key: &str,
    choices: &[&str],
    fallback: &str,
) -> Result<String> {
    let Some(text) = words.option(key) else {
        return Ok(String::from(fallback));
    };
    let folded = text.to_ascii_lowercase();
    if choices.contains(&folded.as_str()) {
        return Ok(folded);
    }
    Err(make_error(
        ErrorCode::InvalidArgument,
        format!("{key}= is {}", choices.join("|")),
        format!("{key}={text}"),
    ))
}

/// "a,b,c" as its items, blanks trimmed and empty items dropped.
pub fn list_of(text: &str) -> Vec<String> {
    text.split(',')
        .map(trimmed_text)
        .filter(|item| !item.is_empty())
        .map(String::from)
        .collect()
}

/// The target of a verb that draws its result: TO LAYER <path>, else
/// `fallback`. Unsupported for any other target, naming the verb.
pub fn layer_target(words: &VerbWords, verb: &str, fallback: &str) -> Result<String> {
    let Some(target) = &words.target else {
        return Ok(String::from(fallback));
    };
    if target.kind != TargetKind::Layer {
        return Err(Error::new(
            ErrorCode::Unsupported,
            format!("{verb} draws its result on a layer: TO LAYER <path>"),
        ));
    }
    Ok(target.name.clone())
}

// ---- the drawing as a feature set -------------------------------------------------------------

/// The scope matched and converted with the project's coordinate system: what
/// every vector verb binds. VIEW is the interpreter's (refused headless).
pub fn bind_scope(context: &Context<'_>, scope: &ScopeWords) -> Result<BoundDrawing> {
    let options = DrawingDatasetOptions {
        crs_wkt: project_crs(context),
        ..DrawingDatasetOptions::default()
    };
    bind_drawing(context, scope, &options)
}

/// The table named `name`, or None.
pub fn table_named<'s>(set: &'s FeatureSet, name: &str) -> Option<&'s FeatureTable> {
    set.tables.iter().find(|table| table.name == name)
}

/// The index of the field `name` in `table`, or None.
pub fn field_index(table: &FeatureTable, name: &str) -> Option<usize> {
    table.fields.iter().position(|field| field.name == name)
}

/// Whether any table of `set` has the field.
pub fn has_field(set: &FeatureSet, name: &str) -> bool {
    set.tables
        .iter()
        .any(|table| field_index(table, name).is_some())
}

/// The entity a feature came from: its katana_id (under `field`, "katana_id"
/// in the C++'s default; layer-algebra writes "input_katana_id").
pub fn feature_id(table: &FeatureTable, feature: &Feature, field: &str) -> Option<EntityId> {
    let index = field_index(table, field)?;
    match feature.values.get(index) {
        Some(FieldValue::Integer64(id)) if *id > 0 => Some(*id as EntityId),
        _ => None,
    }
}

/// One table of every feature of `tables`, called `name`: the fields are the
/// union in first-seen order, a field whose type differs between tables
/// becomes a String. How a result drawn on ONE layer is made from the points,
/// lines and polygons tables an algorithm hands back.
pub fn merged_table(tables: &[&FeatureTable], name: String) -> FeatureTable {
    let mut merged = FeatureTable {
        name,
        ..FeatureTable::default()
    };
    let mut first = true;
    for table in tables {
        if first {
            merged.kind = table.kind;
            first = false;
        } else if merged.kind != table.kind {
            merged.kind = GeometryKind::Unknown;
        }
        merged.has_z = merged.has_z || table.has_z;
        if merged.crs_wkt.is_empty() {
            merged.crs_wkt = table.crs_wkt.clone();
        }
        for field in &table.fields {
            match field_index(&merged, &field.name) {
                None => merged.fields.push(field.clone()),
                Some(at) if merged.fields[at].r#type != field.r#type => {
                    merged.fields[at].r#type = FieldType::String;
                }
                Some(_) => {}
            }
        }
    }
    for table in tables {
        let into: Vec<usize> = table
            .fields
            .iter()
            .map(|field| {
                field_index(&merged, &field.name).expect("every field was merged in above")
            })
            .collect();
        for feature in &table.features {
            let mut copy = Feature {
                parts: feature.parts.clone(),
                values: vec![FieldValue::Null; merged.fields.len()],
            };
            for (&to, value) in into.iter().zip(&feature.values) {
                let text = merged.fields[to].r#type == FieldType::String
                    && !matches!(value, FieldValue::Text(_) | FieldValue::Null);
                copy.values[to] = if text {
                    FieldValue::Text(text_of(value))
                } else {
                    value.clone()
                };
            }
            merged.features.push(copy);
        }
    }
    merged
}

// ---- measures -------------------------------------------------------------------------------

fn plan_ring(ring: &[GeoPoint]) -> Polyline2 {
    Polyline2 {
        vertices: ring
            .iter()
            .map(|point| Point2::new(point.x, point.y))
            .collect(),
        closed: true,
    }
}

/// A ring's area, whichever way round it runs.
pub fn ring_area(ring: &[GeoPoint]) -> f64 {
    // A ring GDAL hands back repeats its first point; the repeat adds nothing.
    plan_ring(ring).area()
}

/// A polygon's area: its exterior less its holes. 0 for anything else.
pub fn geometry_area(geometry: &VectorGeometry) -> f64 {
    if geometry.kind != GeometryKind::Polygon || geometry.parts.is_empty() {
        return 0.0;
    }
    let mut area = ring_area(&geometry.parts[0]);
    for hole in &geometry.parts[1..] {
        area -= ring_area(hole);
    }
    area
}

/// A line's length, or a polygon's perimeter (holes included); 0 for a
/// point.
pub fn geometry_length(geometry: &VectorGeometry) -> f64 {
    if geometry.kind == GeometryKind::Point {
        return 0.0;
    }
    let mut length = 0.0;
    for ring in &geometry.parts {
        for pair in ring.windows(2) {
            length += (pair[1].x - pair[0].x).hypot(pair[1].y - pair[0].y);
        }
        if geometry.kind == GeometryKind::Polygon
            && ring.len() > 2
            && let (Some(front), Some(back)) = (ring.first(), ring.last())
            && (front.x != back.x || front.y != back.y)
        {
            length += (front.x - back.x).hypot(front.y - back.y);
        }
    }
    length
}

pub fn feature_area(feature: &Feature) -> f64 {
    let mut area = 0.0;
    for part in &feature.parts {
        area += geometry_area(part);
    }
    area
}

pub fn feature_length(feature: &Feature) -> f64 {
    let mut length = 0.0;
    for part in &feature.parts {
        length += geometry_length(part);
    }
    length
}

/// Totals over a set, and how many features hold a geometry with no points
/// (GEOS gives an empty polygon for a line buffered inwards).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Measures {
    pub features: usize,
    pub empty: usize,
    pub area: f64,
    pub length: f64,
}

pub fn measure(set: &FeatureSet) -> Measures {
    let mut measures = Measures::default();
    for table in &set.tables {
        for feature in &table.features {
            measures.features += 1;
            if feature.parts.iter().all(empty_geometry) {
                measures.empty += 1;
            }
            measures.area += feature_area(feature);
            measures.length += feature_length(feature);
        }
    }
    measures
}

/// The set without its empty geometries, and without tables left empty.
pub fn without_empty(mut set: FeatureSet) -> FeatureSet {
    for table in &mut set.tables {
        for feature in &mut table.features {
            feature.parts.retain(|part| !empty_geometry(part));
        }
        table.features.retain(|feature| !feature.parts.is_empty());
    }
    set.tables.retain(|table| !table.features.is_empty());
    set
}

// ---- running --------------------------------------------------------------------------------

/// One vector algorithm on `input` (bound to its "input" argument), `tokens`
/// being GDAL's own words, the output kept in memory. Its features - an empty
/// set when it wrote none - and its warnings appended to `warnings`. The
/// C++'s default for `more` is none.
pub fn run_vector(
    path: &[String],
    input: &FeatureSet,
    tokens: &[String],
    stop: Option<&AtomicBool>,
    progress: Progress<'_>,
    warnings: &mut Vec<String>,
    more: &[(String, FeatureSet)],
) -> Result<FeatureSet> {
    let mut values = vec![(
        String::from("input"),
        ArgValue::Dataset(DatasetValue::Features(input.clone())),
    )];
    for (arg, set) in more {
        values.push((
            arg.clone(),
            ArgValue::Dataset(DatasetValue::Features(set.clone())),
        ));
    }
    let request = RunRequest {
        path: path.to_vec(),
        values,
        tokens: tokens.to_vec(),
        output_to: OutputTo::Memory,
        ..RunRequest::default()
    };
    let outputs = gp::run(&request, stop, progress)?;
    for diagnostic in &outputs.diagnostics {
        if !diagnostic.failure {
            warnings.push(diagnostic.message.clone());
        }
    }
    Ok(outputs.features.unwrap_or_default())
}

/// A progress of its own made from a part of another: what progress_span
/// gives, lent on as a `Progress` by `.as_deref()`.
pub type SpanProgress<'p> = Box<dyn Fn(f64) + Sync + 'p>;

/// A progress that reports [from, to) of `progress`: steps run one after
/// another share one bar. None for no progress.
pub fn progress_span(progress: Progress<'_>, from: f64, to: f64) -> Option<SpanProgress<'_>> {
    let progress = progress?;
    let span: SpanProgress<'_> = Box::new(move |fraction: f64| {
        progress(from + (to - from) * cpp::clamp(fraction, 0.0, 1.0));
    });
    Some(span)
}

// ---- applying -------------------------------------------------------------------------------

/// What an apply changed, for the output record.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Applied {
    pub created: usize,
    pub updated: usize,
    pub deleted: usize,
    pub skipped: usize,
    pub created_ids: Vec<EntityId>,
}

/// Executes `command` as ONE undo step (nothing for none), then frames what
/// it created. The ids it created.
pub fn execute_step(
    context: &mut Context<'_>,
    command: Option<CommandPtr>,
) -> Result<Vec<EntityId>> {
    let Some(command) = command else {
        return Ok(Vec::new());
    };
    context.document.execute(command)?;
    let created = context.document.last_created_entities();
    if let Some(frame) = &context.front_end.frame
        && !created.is_empty()
    {
        let mut framed = Box2::default();
        for &id in &created {
            if let Some(entity) = context.document.model().entities.find(id) {
                let one = bounding_box(&entity.geometry);
                if !one.empty() {
                    framed.expand(one.min);
                    framed.expand(one.max);
                }
            }
        }
        if !framed.empty() {
            frame(&framed);
        }
    }
    Ok(created)
}

/// `output arg=output kind=vector target=layer layer=<layer> created=
/// updated= deleted= skipped=` - the record apply_outputs writes, so a
/// curated verb's reply reads as the GDAL verb's does. An empty layer is an
/// in-place apply: target=in-place, no layer.
pub fn output_record(layer: &str, applied: &Applied) -> String {
    let target = if layer.is_empty() {
        String::from("target=in-place")
    } else {
        format!("target=layer layer={}", value(layer))
    };
    format!(
        "output arg=output kind=vector {target} created={} updated={} deleted={} skipped={}",
        applied.created, applied.updated, applied.deleted, applied.skipped
    )
}

/// `gis op=<op> seconds=<s> cancelled=no`: a curated verb's first record.
pub fn gis_record(op: &str, seconds: f64) -> String {
    format!(
        "gis op={} seconds={} cancelled=no",
        value(op),
        fixed3(seconds)
    )
}

/// A scope that took nothing, or a preview: said, nothing run.
pub fn not_run_record(op: &str, why: &str) -> String {
    let mut record = format!("gis op={} seconds=0.000 cancelled=no ran=no", value(op));
    if !why.is_empty() {
        record.push(' ');
        record += why;
    }
    record
}

/// The records, one per line, an empty one left out.
pub fn joined(records: &[String]) -> String {
    let mut text = String::new();
    for record in records {
        if !record.is_empty() {
            if !text.is_empty() {
                text.push('\n');
            }
            text += record;
        }
    }
    text
}

/// A prepared line that answers at once.
pub fn answered(title: impl Into<String>, reply: impl Into<String>) -> Prepared {
    Prepared {
        title: title.into(),
        reply: Some(reply.into()),
        work: None,
    }
}

/// The entities `ids` names, copied: what an in-place apply compares the
/// drawing with (unchanged_since) before it changes anything.
pub fn copies_of(document: &Document, ids: &[EntityId]) -> Vec<Entity> {
    ids.iter()
        .filter_map(|&id| document.model().entities.find(id).cloned())
        .collect()
}

/// A value of a feature's field as an entity property; None for a null.
pub fn property_of(value: &FieldValue) -> Option<PropertyValue> {
    match value {
        FieldValue::Null => None,
        FieldValue::Boolean(flag) => Some(PropertyValue::Boolean(*flag)),
        FieldValue::Integer64(whole) => Some(PropertyValue::Integer(*whole)),
        FieldValue::Real(real) => Some(PropertyValue::Real(*real)),
        FieldValue::Text(text) => Some(PropertyValue::Text(text.clone())),
    }
}

// ---- rows: what a query or an overlay reports ---------------------------------------------------

/// One feature's fields as a reply's row: a field whose value is null has no
/// cell, so a row record says nothing of it rather than an empty value.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Row {
    pub cells: Vec<(String, String)>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RowOptions {
    /// katana_id as entity=, gis.with as with=: an overlay's pieces.
    pub entity_keys: bool,
    /// The area= of an area, the length= of a line.
    pub measures: bool,
}

/// A value as a row writes it: 12, 0.5, true, text as it is; empty for null.
pub fn cell_text(value: &FieldValue) -> String {
    text_of(value)
}

/// A column's name as a row's key: a blank, '=' or '"' - which would break
/// the record - becomes '_' ("count(*) n" is count(*)_n). The column record
/// says which name each key stands for.
pub fn row_key(name: &str) -> String {
    let key: String = name
        .chars()
        .map(|c| {
            if matches!(c, ' ' | '\t' | '=' | '"') {
                '_'
            } else {
                c
            }
        })
        .collect();
    if key.is_empty() {
        String::from("_")
    } else {
        key
    }
}

/// The C++'s default options are both off.
pub fn rows_of(table: &FeatureTable, options: &RowOptions) -> Vec<Row> {
    let mut rows = Vec::new();
    for feature in &table.features {
        let mut row = Row::default();
        for (field, value) in table.fields.iter().zip(&feature.values) {
            if *value == FieldValue::Null {
                continue;
            }
            let mut key = row_key(&field.name);
            if options.entity_keys {
                if key == "katana_id" {
                    key = String::from("entity");
                } else if key == "gis.with" {
                    key = String::from("with");
                }
            }
            row.cells.push((key, cell_text(value)));
        }
        if options.measures {
            let has = |kind: GeometryKind| feature.parts.iter().any(|part| part.kind == kind);
            if has(GeometryKind::Polygon) {
                row.cells
                    .push((String::from("area"), fixed3(feature_area(feature))));
            } else if has(GeometryKind::LineString) {
                row.cells
                    .push((String::from("length"), fixed3(feature_length(feature))));
            }
        }
        rows.push(row);
    }
    rows
}

/// `row <field>=<value> ...`
pub fn row_record(row: &Row) -> String {
    let mut record = String::from("row");
    for (key, text) in &row.cells {
        record += &format!(" {key}={}", value(text));
    }
    record
}

/// `column name=<field> key=<its row key> type=string|integer|real|boolean|date|datetime`,
/// one per field: what a reader of the rows needs to know a 7 from a "7".
pub fn column_records(table: &FeatureTable) -> Vec<String> {
    table
        .fields
        .iter()
        .map(|field| {
            let kind = match field.r#type {
                FieldType::Boolean => "boolean",
                FieldType::Integer64 => "integer",
                FieldType::Real => "real",
                FieldType::Date => "date",
                FieldType::DateTime => "datetime",
                FieldType::String => "string",
            };
            format!(
                "column name={} key={} type={kind}",
                value(&field.name),
                value(&row_key(&field.name))
            )
        })
        .collect()
}

fn csv_cell(text: &str) -> String {
    if !text.contains([',', '"', '\r', '\n']) {
        return String::from(text);
    }
    let mut quoted = String::from("\"");
    for c in text.chars() {
        if c == '"' {
            quoted += "\"\"";
        } else {
            quoted.push(c);
        }
    }
    quoted + "\""
}

/// The rows as CSV: `columns` in that order, or as the rows first name them
/// when empty; a cell quoted when it holds a comma, a quote or a line break.
pub fn write_csv(path: &str, rows: &[Row], columns: &[String]) -> Status {
    let mut columns: Vec<String> = columns.to_vec();
    if columns.is_empty() {
        for row in rows {
            for (key, _) in &row.cells {
                if !columns.contains(key) {
                    columns.push(key.clone());
                }
            }
        }
    }
    let header: Vec<String> = columns
        .iter()
        .map(|column| csv_cell(column.as_str()))
        .collect();
    let mut text = header.join(",");
    text.push('\n');
    for row in rows {
        let mut line = String::new();
        for (c, column) in columns.iter().enumerate() {
            let mut cell = "";
            for (key, value) in &row.cells {
                if key == column {
                    cell = value.as_str();
                }
            }
            if c != 0 {
                line.push(',');
            }
            line += &csv_cell(cell);
        }
        text += &line;
        text.push('\n');
    }
    if std::fs::write(path_from_text(path), text).is_err() {
        return Err(make_error(
            ErrorCode::FileExportFailure,
            "cannot write the rows",
            path,
        ));
    }
    Ok(())
}

/// TO REPORT: every table's columns and rows, then
/// `output arg=<arg> kind=vector target=report rows=<n>`.
pub fn report_records(set: &FeatureSet, arg: &str) -> Vec<String> {
    let mut records = Vec::new();
    let mut count = 0;
    for table in &set.tables {
        records.extend(column_records(table));
        for row in rows_of(table, &RowOptions::default()) {
            records.push(row_record(&row));
            count += 1;
        }
    }
    records.push(format!(
        "output arg={} kind=vector target=report rows={count}",
        value(arg)
    ));
    records
}

/// TO SELECTION: the entities the katana_id column names become the
/// selection (the drawing's own; not an undo step, as SELECT is not); the
/// record `output arg=<arg> kind=vector target=selection selected=<n>
/// missing=<m>`. InvalidArgument when no table has a katana_id column to
/// select by.
pub fn select_features(context: &mut Context<'_>, set: &FeatureSet, arg: &str) -> Result<String> {
    let mut has_ids = false;
    let mut present: Vec<EntityId> = Vec::new();
    let mut missing = 0;
    for table in &set.tables {
        if field_index(table, "katana_id").is_none() {
            continue;
        }
        has_ids = true;
        for feature in &table.features {
            match feature_id(table, feature, "katana_id") {
                Some(id) if context.document.model().entities.contains(id) => {
                    if !present.contains(&id) {
                        present.push(id);
                    }
                }
                _ => missing += 1,
            }
        }
    }
    if !has_ids && !set.tables.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "a selection is made from the katana_id column: the result has none; SELECT \
             katana_id FROM ...",
        ));
    }
    let selected = present.len();
    context.document.selection_mut().set(present);
    context.document.notify_selection_changed();
    Ok(format!(
        "output arg={} kind=vector target=selection selected={selected} missing={missing}",
        value(arg)
    ))
}

/// A statement as one quoted word of a line: its line breaks as blanks, and -
/// for SQLite - its double-quoted identifiers as [bracketed] ones, which
/// SQLite reads alike and a line can carry ("gis.source" is [gis.source]).
/// InvalidArgument for a double quote no line can carry: one inside a '...'
/// literal, one never closed, or any in OGR SQL. What the SQL dialog and
/// katana_gis_query write their GIS SQL lines with.
pub fn sql_for_line(sql: &str, sqlite: bool) -> Result<String> {
    let mut out = String::new();
    let mut literal = false;
    let mut identifier = false;
    for c in sql.chars() {
        // A line has one line.
        let c = if matches!(c, '\r' | '\n' | '\t') {
            ' '
        } else {
            c
        };
        if identifier {
            out.push(if c == '"' { ']' } else { c });
            identifier = c != '"';
            continue;
        }
        if c == '\'' {
            literal = !literal;
        } else if c == '"' {
            if !sqlite {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    "OGR SQL's double-quoted identifiers cannot be carried by a command line; \
                     the sqlite dialect takes [brackets]",
                ));
            }
            if literal {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    "a double quote inside a '...' literal cannot be carried by a command line",
                ));
            }
            out.push('[');
            identifier = true;
            continue;
        }
        out.push(c);
    }
    if identifier {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "a double-quoted identifier in the statement is not closed",
        ));
    }
    Ok(String::from(trimmed_text(&out)))
}

// ---- reshaping entities in place ----------------------------------------------------------------

/// A ring without the repeat of its first point GDAL closes it with.
fn open_ring(ring: &[GeoPoint]) -> Vec<GeoPoint> {
    let mut points = ring.to_vec();
    if points.len() > 1
        && let (Some(front), Some(back)) = (points.first(), points.last())
        && front.x == back.x
        && front.y == back.y
    {
        points.pop();
    }
    points
}

/// Coordinates pass through GDAL's MEM datasets as doubles, so a vertex an
/// algorithm left alone comes back bit for bit; the tolerance is the model's
/// geometric one, for the arithmetic of one that was moved and put back.
fn same_point(a: &GeoPoint, b: &GeoPoint) -> bool {
    (a.x - b.x).abs() <= GEOMETRIC && (a.y - b.y).abs() <= GEOMETRIC
}

fn same_holes(before: &[Vec<GeoPoint>], after: &[Vec<GeoPoint>]) -> bool {
    if before.len() != after.len() {
        return false;
    }
    let mut used = vec![false; after.len()];
    for hole in before {
        let mut found = false;
        for (h, other) in after.iter().enumerate() {
            if found {
                break;
            }
            if !used[h] && same_ring(hole, other) {
                used[h] = true;
                found = true;
            }
        }
        if !found {
            return false;
        }
    }
    true
}

fn holes_of(polygon: &VectorGeometry) -> &[Vec<GeoPoint>] {
    if polygon.parts.len() > 1 {
        &polygon.parts[1..]
    } else {
        &[]
    }
}

fn signed_ring_area(ring: &[GeoPoint]) -> f64 {
    plan_ring(&open_ring(ring)).signed_area()
}

fn heights_of(points: &[GeoPoint], has_z: bool) -> Vec<Option<f64>> {
    points
        .iter()
        .map(|point| (has_z && point.z.is_finite()).then_some(point.z))
        .collect()
}

/// A part as a copy of `from`: a ring as a closed polyline, a line as a line
/// or polyline.
fn copy_with(from: &Entity, points: &[GeoPoint], ring: bool, has_z: bool) -> Entity {
    let mut made = from.clone();
    made.id = INVALID_ENTITY_ID;
    let used = if ring {
        open_ring(points)
    } else {
        points.to_vec()
    };
    made.geometry = if !ring && used.len() == 2 {
        Geometry::Line(Segment2::new(
            Point2::new(used[0].x, used[0].y),
            Point2::new(used[1].x, used[1].y),
        ))
    } else {
        Geometry::Polyline(Polyline2 {
            vertices: used
                .iter()
                .map(|point| Point2::new(point.x, point.y))
                .collect(),
            closed: ring,
        })
    };
    set_heights(&mut made.properties, &heights_of(&used, has_z));
    made
}

/// The features of `set` by the entity each came from (the first of each).
pub fn features_by_id(set: &FeatureSet) -> BTreeMap<EntityId, &Feature> {
    let mut by_id = BTreeMap::new();
    for table in &set.tables {
        for feature in &table.features {
            if let Some(id) = feature_id(table, feature, "katana_id") {
                by_id.entry(id).or_insert(feature);
            }
        }
    }
    by_id
}

/// Whether two rings are the same ring: the same points in the same cyclic
/// order, either way round, from any start, a closing repeat ignored.
pub fn same_ring(a: &[GeoPoint], b: &[GeoPoint]) -> bool {
    let first = open_ring(a);
    let second = open_ring(b);
    if first.len() != second.len() {
        return false;
    }
    let n = first.len();
    for start in 0..n {
        if !same_point(&first[0], &second[start]) {
            continue;
        }
        let mut forward = true;
        let mut backward = true;
        let mut i = 0;
        while i < n && (forward || backward) {
            forward = forward && same_point(&first[i], &second[(start + i) % n]);
            backward = backward && same_point(&first[i], &second[(start + n - i) % n]);
            i += 1;
        }
        if forward || backward {
            return true;
        }
    }
    n == 0
}

/// Whether an algorithm gave a feature back as it was: every part of the same
/// kind with the same rings (holes in any order), or the same line.
pub fn same_shape(before: &Feature, after: &Feature) -> bool {
    if before.parts.len() != after.parts.len() {
        return false;
    }
    for (was, now) in before.parts.iter().zip(&after.parts) {
        if was.kind != now.kind || was.parts.is_empty() != now.parts.is_empty() {
            return false;
        }
        if was.parts.is_empty() {
            continue;
        }
        if was.kind == GeometryKind::Polygon {
            if !same_ring(&was.parts[0], &now.parts[0]) || !same_holes(holes_of(was), holes_of(now))
            {
                return false;
            }
            continue;
        }
        let line = &was.parts[0];
        let other = &now.parts[0];
        if line.len() != other.len() {
            return false;
        }
        if line
            .iter()
            .zip(other)
            .any(|(point, other)| !same_point(point, other))
        {
            return false;
        }
    }
    true
}

/// An entity's new shape: the parts an algorithm gave back for the feature it
/// came from (make-valid, clean-coverage, clip), and what it was.
#[derive(Clone, Debug, PartialEq)]
pub struct Reshaped {
    pub id: EntityId,
    pub parts: Vec<VectorGeometry>,
    pub before: Feature,
}

impl Default for Reshaped {
    fn default() -> Self {
        Self {
            id: INVALID_ENTITY_ID,
            parts: Vec::new(),
            before: Feature::default(),
        }
    }
}

/// One entity whose result was several parts: its id is kept on the largest
/// and the rest are made beside it, as copies of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Split {
    pub id: EntityId,
    pub parts: usize,
    /// Entities made: a part's rings are one each.
    pub made: usize,
}

impl Default for Split {
    fn default() -> Self {
        Self {
            id: INVALID_ENTITY_ID,
            parts: 0,
            made: 0,
        }
    }
}

#[derive(Default)]
pub struct Reshape {
    /// None when nothing changes.
    pub command: Option<CommandPtr>,
    pub updated: usize,
    pub created: usize,
    /// Entities left as they were: an area whose holes the result changed (an
    /// entity is one ring; its holes are entities of their own), or one the
    /// drawing no longer has.
    pub left: usize,
    pub splits: Vec<Split>,
    pub warnings: Vec<String>,
}

/// The one command that gives each entity of `reshaped` its new shape: the
/// largest part replaces its geometry (set_entity_geometry, so its id - and
/// its labels and associations - are kept; an area keeps the way round it
/// ran), and every other part is a new entity copied from it - layer, style,
/// colour, properties - with gis.op=<op> and gis.source=<its id>, a part with
/// holes as rings tagged gis.ring and gis.part. The created entities come in
/// the order of `splits`, each split's parts after the largest in order.
pub fn reshape_command(
    model: &Model,
    reshaped: &[Reshaped],
    op: &str,
    command_name: &str,
) -> Result<Reshape> {
    let mut out = Reshape::default();
    let mut table = FeatureTable {
        name: String::from("reshaped"),
        fields: vec![FieldDef {
            name: String::from("katana_id"),
            r#type: FieldType::Integer64,
        }],
        ..FeatureTable::default()
    };
    let mut made: Vec<Entity> = Vec::new();
    // The id the first made entity will have (ids are monotonic and the
    // command makes them in order), so a part's rings name their exterior.
    let first_id = model.entities.next_id();
    let mut emptied = 0;
    let mut holes_changed = 0;
    let mut missing = 0;
    let mut flattened = 0;
    for one in reshaped {
        let Some(entity) = model.entities.find(one.id) else {
            missing += 1;
            continue;
        };
        let kind = one
            .before
            .parts
            .first()
            .map_or(GeometryKind::Unknown, |part| part.kind);
        let parts: Vec<&VectorGeometry> = one
            .parts
            .iter()
            .filter(|part| part.kind == kind && !empty_geometry(part))
            .collect();
        if parts.is_empty() {
            emptied += 1;
            continue;
        }
        let size = |part: &VectorGeometry| {
            if kind == GeometryKind::Polygon {
                geometry_area(part)
            } else {
                geometry_length(part)
            }
        };
        let mut best = 0;
        for (p, part) in parts.iter().enumerate().skip(1) {
            if size(part) > size(parts[best]) {
                best = p;
            }
        }
        let mut kept = parts[best].clone();
        if kind == GeometryKind::Polygon {
            // An area is one ring and its holes entities of their own, which
            // stay: a result that changed the holes cannot be written on it.
            if !same_holes(holes_of(&one.before.parts[0]), holes_of(&kept)) {
                holes_changed += 1;
                continue;
            }
            kept.parts.truncate(1);
            // The winding the entity had, read from its ring as the one
            // conversion made it: the same for a polyline, a curve polyline,
            // an ellipse or a spline, where a Polyline2-only reading took
            // every other kind as anticlockwise.
            let was_ring = &one.before.parts[0].parts;
            let was = was_ring
                .first()
                .map_or(1.0, |ring| signed_ring_area(ring.as_slice()));
            if (was < 0.0) != (signed_ring_area(&kept.parts[0]) < 0.0) {
                kept.parts[0].reverse();
            }
        }
        let had_heights = one.before.parts.iter().any(|part| part.has_z);
        flattened += usize::from(had_heights && !kept.has_z);
        table.features.push(Feature {
            parts: vec![kept],
            values: vec![FieldValue::Integer64(one.id as i64)],
        });
        if parts.len() == 1 {
            continue;
        }
        let mut split = Split {
            id: one.id,
            parts: parts.len(),
            made: 0,
        };
        for (p, part) in parts.iter().enumerate() {
            if p == best {
                continue;
            }
            let exterior = made.len();
            for (r, ring) in part.parts.iter().enumerate() {
                let mut copy = copy_with(entity, ring, kind == GeometryKind::Polygon, part.has_z);
                if part.parts.len() > 1 {
                    copy.properties.insert(
                        String::from("gis.ring"),
                        PropertyValue::Text(String::from(if r == 0 { "exterior" } else { "hole" })),
                    );
                    copy.properties.insert(
                        String::from("gis.part"),
                        PropertyValue::Integer((first_id + exterior as EntityId) as i64),
                    );
                }
                copy.properties.insert(
                    String::from("gis.op"),
                    PropertyValue::Text(String::from(op)),
                );
                copy.properties.insert(
                    String::from("gis.source"),
                    PropertyValue::Integer(one.id as i64),
                );
                made.push(copy);
                split.made += 1;
                if kind != GeometryKind::Polygon {
                    break; // a line has one part
                }
            }
        }
        out.splits.push(split);
    }
    let options = ResultOptions {
        mode: ResultMode::UpdateGeometry,
        command_name: String::from(command_name),
        ..ResultOptions::default()
    };
    let updates = FeatureSet {
        tables: vec![table],
    };
    let mut plan = result_command(model, &updates, &options)?;
    out.updated = plan.updated;
    out.created = made.len();
    out.left = emptied + holes_changed + missing + plan.skipped;
    out.warnings.append(&mut plan.warnings);
    if emptied != 0 {
        out.warnings.push(format!(
            "{emptied} entities came back with nothing of their kind and were left as they were"
        ));
    }
    if holes_changed != 0 {
        out.warnings.push(format!(
            "{holes_changed} areas came back with other holes; an area and its holes are \
             entities of their own, so they were left as they were"
        ));
    }
    if missing != 0 {
        out.warnings.push(format!(
            "{missing} results name an entity the drawing no longer has"
        ));
    }
    if flattened != 0 {
        out.warnings.push(format!(
            "{flattened} entities came back without heights and are now in plan"
        ));
    }
    let command = plan.command.take();
    if command.is_none() && made.is_empty() {
        return Ok(out);
    }
    let mut transaction = Transaction::new(command_name);
    if let Some(command) = command {
        transaction.add(command);
    }
    if !made.is_empty() {
        transaction.add(create_entities(made));
    }
    out.command = Some(Box::new(transaction));
    Ok(out)
}

/// `split entity=<id> parts=<n> created=<id,id>`: which entities a reshape
/// made from which, `created` being the ids the command made, in its order.
pub fn split_records(splits: &[Split], created: &[EntityId]) -> Vec<String> {
    let mut records = Vec::new();
    let mut at = 0;
    for split in splits {
        let mut ids = Vec::new();
        let mut i = 0;
        while i < split.made && at < created.len() {
            ids.push(format!("{}", created[at]));
            i += 1;
            at += 1;
        }
        records.push(format!(
            "split entity={} parts={} kept={} created={}",
            split.id,
            split.parts,
            split.id,
            ids.join(",")
        ));
    }
    records
}

/// Markers of a check (GIS CHECK, GIS COVERAGE CHECK) on `layer`, replacing
/// the markers of the same `kind` a previous run left there - as AUTOLABEL
/// replaces its rule's labels - in ONE step: the old ones deleted, the layer
/// made where missing, the new ones created, each tagged gis.marker=<kind>.
/// None when there is nothing to delete or create. `removed` is how many
/// stale markers the command deletes.
pub fn replace_markers(
    model: &Model,
    layer: &str,
    kind: &str,
    markers: Vec<Entity>,
    command_name: &str,
    removed: &mut usize,
) -> Option<CommandPtr> {
    let mut stale = Vec::new();
    model.entities.for_each(|entity| {
        if entity.layer == layer
            && entity
                .properties
                .get("gis.marker")
                .is_some_and(|tag| property_to_string(tag) == kind)
        {
            stale.push(entity.id);
        }
    });
    *removed = stale.len();
    if stale.is_empty() && markers.is_empty() {
        return None;
    }
    let mut transaction = Transaction::new(command_name);
    if !stale.is_empty() {
        transaction.add(delete_entities(stale));
    }
    if !markers.is_empty() {
        if !model.layers.contains(layer) {
            transaction.add(create_layer(Layer {
                name: String::from(layer),
                ..Layer::default()
            }));
        }
        let markers = markers
            .into_iter()
            .map(|mut marker| {
                marker.layer = String::from(layer);
                marker.properties.insert(
                    String::from("gis.marker"),
                    PropertyValue::Text(String::from(kind)),
                );
                marker
            })
            .collect();
        transaction.add(create_entities(markers));
    }
    Some(Box::new(transaction))
}
