//! The drawing as the schedule (docs/subsurface_utilities.md, "Drawing data";
//! include/katana/cad/utilities/utility_data.hpp,
//! src/katana_cad/utilities/utility_data.cpp): what UTILITY DRAW put in the
//! document, read back, so that every UTILITY verb acts on what is drawn as
//! well as on a schedule file - by the shared scope and filter words
//! (scope_verbs.rs), as Global Modify acts.
//!
//! A drawn point carries its whole row of the schedule: its place along its
//! line, the geometry that is its position, its level, depth and their
//! reference, surface, method, uncertainties, claimed level, what it
//! verifies, the path to the next point and the schema's own fields, and its
//! line's attributes (utility_drawing.rs, keys). So the POINTS ARE THE
//! SCHEDULE: a point moved in CAD, or a method or a level edited in the
//! property panel, is the schedule edited. The runs are derived output -
//! UTILITY REGRADE draws them again from the points.
//!
//! A scope that takes part of a line takes the WHOLE line, because the
//! grading is per line: a run of it, one of its points, or anything else
//! carrying its id brings in every point of it in the drawing, and the reply
//! says how many lines were completed so. What carries no utility data is
//! ignored, and counted. A line whose points do not agree - two at one place
//! along it, one missing its place or its method, two that give its owner
//! differently - is refused by name: a guess at which point is right would be
//! a schedule nobody wrote.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use katana_commands::{
    ChangeSet, ChangeSetCommand, CommandContext, CommandPtr, Transaction, create_layer,
    create_linetype,
};
use katana_core::text::{
    equals_ignoring_case, format_exact_real, is_ascii_space, parse_finite_double,
};
use katana_core::{Error, ErrorCode, Result, Status};
use katana_entity::entity::{heights_of, property_to_string};
use katana_entity::layer_path::{layer_ancestors, layer_depth, layer_leaf, layer_parent};
use katana_entity::{
    Alignment, Entity, EntityId, EntityType, EnumName, Geometry, Layer, Linetype, Model,
    PointGeometry, PropertyMap, PropertyValue,
};
use katana_geometry::alignment::SolvedAlignment;
use katana_geometry::chording::sagitta_chord_count;
use katana_geometry::curves2d::CURVE_CHORD_TOLERANCE;
use katana_geometry::profile::{SolvedProfile, solve_profile};
use katana_geometry::{AlignmentShape, Point2, ProfileElementKind};
use katana_math::vec2::distance;
use katana_math::{cpp, tolerance};
use katana_survey::Coordinate2;
use katana_survey::subsurface::{
    self as sub, CarriedOn, DesignAlignment, DesignVertex, LevelReference, LocationMethod,
    PathEvidence, QualityLevel, UtilityAttributes, UtilityLine, UtilityStatus, UtilityType,
    UtilityVertex,
};

use super::utility_drawing::{
    DEFAULT_UTILITY_LAYER_PREFIX, UTILITY_REGRADE_STEP, UtilityDrawOptions, UtilityDrawing,
    draw_utilities, keys, utility_type_word,
};
use crate::survey_coding::{code_property_candidates, survey_code_of};
use crate::survey_import::SurveyImportOptions;

/// One service as the drawing holds it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DrawnService {
    pub line: UtilityLine,
    /// Its points in order along it: line.vertices[i] was read from points[i].
    pub points: Vec<EntityId>,
    /// Its runs: the polylines carrying its id, ascending.
    pub runs: Vec<EntityId>,
    /// The scope took some, not all, of its points - or none, only a run.
    pub completed: bool,
    /// The settings it was graded with when drawn (keys::SPACING,
    /// keys::MINIMUM_COVER); None when its points do not say.
    pub spacing: Option<f64>,
    pub minimum_cover: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct UtilityData {
    /// In the order drawn: by the id of each service's first point, which is
    /// schedule order for services one UTILITY DRAW made.
    pub services: Vec<DrawnService>,
    /// What the scope took that carries no utility data.
    pub ignored: usize,
    /// Services the scope took part of, read whole.
    pub completed: usize,
}

impl UtilityData {
    pub fn lines(&self) -> Vec<UtilityLine> {
        self.services
            .iter()
            .map(|service| service.line.clone())
            .collect()
    }
}

// The line attributes every point of a line carries, and must agree on -
// with the settings the line was graded with.
const LINE_KEYS: [&str; 10] = [
    keys::TYPE,
    keys::OWNER,
    keys::MATERIAL,
    keys::DIAMETER,
    keys::DIAMETER_INSIDE,
    keys::CONFIGURATION,
    keys::DESCRIPTION,
    keys::STATUS,
    keys::SPACING,
    keys::MINIMUM_COVER,
];

// Every key UTILITY DRAW writes: what REGRADE replaces on a point. Another
// "utility." property - a person's own note - is theirs, and stays.
const DRAWN_KEYS: &[&str] = &[
    keys::LINE,
    keys::TYPE,
    keys::QUALITY_LEVEL,
    keys::OWNER,
    keys::MATERIAL,
    keys::DIAMETER,
    keys::DIAMETER_INSIDE,
    keys::CONFIGURATION,
    keys::DESCRIPTION,
    keys::STATUS,
    keys::LIMITED_BY,
    keys::LENGTH,
    keys::FROM,
    keys::TO,
    keys::VERTEX,
    keys::ORDER,
    keys::METHOD,
    keys::CLAIMED,
    keys::OVER_CLAIM,
    keys::LEVEL,
    keys::DEPTH,
    keys::HORIZONTAL_UNCERTAINTY,
    keys::VERTICAL_UNCERTAINTY,
    keys::PATH,
    keys::SERVICE_LEVEL,
    keys::LEVEL_REFERENCE,
    keys::LEVEL_QUALIFIED,
    keys::SURFACE_LEVEL,
    keys::COVER,
    keys::COVER_NOTE,
    keys::COVER_BELOW_MINIMUM,
    keys::VERIFIES,
    keys::SPACING,
    keys::MINIMUM_COVER,
];

// The C++ `core::trimmed` of text: the ASCII blanks core's is_ascii_space
// names (vertical tab among them, which str::trim_ascii leaves), so a
// UTF-8 word stays UTF-8.
pub(crate) fn trimmed_text(text: &str) -> &str {
    text.trim_matches(|c: char| c.is_ascii() && is_ascii_space(c as u8))
}

fn is_drawn_key(key: &str) -> bool {
    key.starts_with(keys::FIELD_PREFIX)
        || key.starts_with(keys::RECORDED_PREFIX)
        || DRAWN_KEYS.contains(&key)
}

// A cell kept as the schedule wrote it that is the line's, not a vertex's:
// the schedule reader keeps type, status and size per service, and ql and
// level_ref per vertex (UtilityAttributes::recorded).
fn is_line_recorded(column: &str) -> bool {
    column == "type" || column == "status" || column == "size"
}

// The kept cells of `point` that are the line's (`line`) or its own.
fn recorded_of(point: &Entity, line: bool) -> BTreeMap<String, String> {
    let mut recorded = BTreeMap::new();
    for (key, value) in &point.properties {
        let Some(column) = key.strip_prefix(keys::RECORDED_PREFIX) else {
            continue;
        };
        if is_line_recorded(column) == line {
            recorded
                .entry(String::from(column))
                .or_insert_with(|| property_to_string(value));
        }
    }
    recorded
}

// A kept field the schedule format carries per service, as it did when the
// schedule was read. One it does not know is a point's own.
fn is_line_field(name: &str) -> bool {
    sub::utility_csv_columns()
        .iter()
        .any(|column| column.name == name && column.carried == CarriedOn::Line)
}

fn value_of<'a>(entity: &'a Entity, key: &str) -> Option<&'a PropertyValue> {
    entity.properties.get(key)
}

fn text_value(value: &str) -> PropertyValue {
    PropertyValue::Text(String::from(value))
}

// Reads one point of one line, and says which when it cannot.
struct PointReader<'a> {
    line: &'a str,
    point: &'a Entity,
    // What to call it instead, when what is read is not a drawn point: the
    // line entity a service was read from ("polyline (#12)").
    label: String,
}

impl<'a> PointReader<'a> {
    fn new(line: &'a str, point: &'a Entity) -> Self {
        Self {
            line,
            point,
            label: String::new(),
        }
    }

    fn labelled(line: &'a str, point: &'a Entity, label: String) -> Self {
        Self { line, point, label }
    }

    fn name(&self) -> String {
        if !self.label.is_empty() {
            return self.label.clone();
        }
        let vertex = value_of(self.point, keys::VERTEX)
            .map(|vertex| format!("{} ", property_to_string(vertex)))
            .unwrap_or_default();
        format!("point {vertex}(#{})", self.point.id)
    }

    fn refuse(&self, what: &str) -> Error {
        Error::new(
            ErrorCode::InvalidArgument,
            format!("line {}: {} {what}", self.line, self.name()),
        )
    }

    // Text as the property panel shows it, whatever the value's type.
    fn text(&self, key: &str) -> Option<String> {
        value_of(self.point, key).map(property_to_string)
    }

    // A real, a whole number, or text that reads as a number.
    fn number(&self, key: &str) -> Result<Option<f64>> {
        let Some(value) = value_of(self.point, key) else {
            return Ok(None);
        };
        let number = match value {
            PropertyValue::Real(real) => Some(*real),
            PropertyValue::Integer(whole) => Some(*whole as f64),
            PropertyValue::Text(words) => parse_finite_double(trimmed_text(words)),
            PropertyValue::Boolean(_) => None,
        };
        match number.filter(|number| number.is_finite()) {
            Some(number) => Ok(Some(number)),
            None => Err(self.refuse(&format!(
                "has {key} \"{}\", which is not a number",
                property_to_string(value)
            ))),
        }
    }

    fn flag(&self, key: &str) -> Result<bool> {
        let Some(value) = value_of(self.point, key) else {
            return Ok(false);
        };
        if let PropertyValue::Boolean(on) = value {
            return Ok(*on);
        }
        let word = property_to_string(value).to_ascii_lowercase();
        match word.as_str() {
            "true" | "yes" | "1" => Ok(true),
            "false" | "no" | "0" => Ok(false),
            _ => Err(self.refuse(&format!("has {key} \"{word}\", which is not true or false"))),
        }
    }
}

// The line's attributes as one point carries them.
fn line_attributes_of(point: &Entity) -> PropertyMap {
    let mut attributes = PropertyMap::new();
    for key in LINE_KEYS {
        if let Some(value) = value_of(point, key) {
            attributes.insert(String::from(key), value.clone());
        }
    }
    for (key, value) in &point.properties {
        let field = key
            .strip_prefix(keys::FIELD_PREFIX)
            .is_some_and(is_line_field);
        let recorded = key
            .strip_prefix(keys::RECORDED_PREFIX)
            .is_some_and(is_line_recorded);
        if field || recorded {
            attributes
                .entry(key.clone())
                .or_insert_with(|| value.clone());
        }
    }
    attributes
}

// A point that is a vertex of a schedule - it has a point id or a place along
// a line - read with no utility.line, or an empty one.
fn is_stray_vertex(entity: &Entity) -> bool {
    entity.r#type() == EntityType::Point
        && (value_of(entity, keys::VERTEX).is_some() || value_of(entity, keys::ORDER).is_some())
}

// Refuses a stray vertex by its point id and entity id, and names the lines it
// is most likely a point of.
fn stray_refusal(point: &Entity, lines: &[String]) -> Error {
    let vertex = value_of(point, keys::VERTEX)
        .map(|vertex| format!("{} ", property_to_string(vertex)))
        .unwrap_or_default();
    let mut text = format!(
        "point {vertex}(#{}) is a vertex of a schedule and has no {}, the line it is a point of",
        point.id,
        keys::LINE
    );
    if !lines.is_empty() {
        text += &format!(
            "; by its attributes it is a point of line {}",
            lines.join(" or ")
        );
    }
    Error::new(
        ErrorCode::InvalidArgument,
        format!(
            "{text}; give it its line again, or delete its {} and {}",
            keys::VERTEX,
            keys::ORDER
        ),
    )
}

// Every point must say what the first says of its line.
fn agree(
    first: &PointReader<'_>,
    expected: &PropertyMap,
    other: &PointReader<'_>,
    actual: &PropertyMap,
) -> Status {
    let quoted = |value: &PropertyValue| format!("\"{}\"", property_to_string(value));
    for (key, value) in expected {
        match actual.get(key) {
            None => {
                return Err(other.refuse(&format!(
                    "has no {key} where {} has {}; the points of one line must agree",
                    first.name(),
                    quoted(value)
                )));
            }
            Some(found) if found != value => {
                return Err(other.refuse(&format!(
                    "has {key} {} where {} has {}; the points of one line must agree",
                    quoted(found),
                    first.name(),
                    quoted(value)
                )));
            }
            Some(_) => {}
        }
    }
    for (key, value) in actual {
        if !expected.contains_key(key) {
            return Err(other.refuse(&format!(
                "has {key} {} where {} has none; the points of one line must agree",
                quoted(value),
                first.name()
            )));
        }
    }
    Ok(())
}

fn read_attributes(reader: &PointReader<'_>) -> Result<UtilityAttributes> {
    let Some(type_text) = reader.text(keys::TYPE) else {
        return Err(reader.refuse(&format!("has no {}", keys::TYPE)));
    };
    let Some(r#type) = sub::parse_utility_type(&type_text) else {
        return Err(reader.refuse(&format!(
            "has {} \"{type_text}\", which is not a utility type",
            keys::TYPE
        )));
    };
    let mut attributes = UtilityAttributes {
        r#type,
        ..UtilityAttributes::default()
    };
    if let Some(status) = reader.text(keys::STATUS) {
        let Some(parsed) = sub::parse_utility_status(&status) else {
            return Err(reader.refuse(&format!(
                "has {} \"{status}\", which is not a utility status",
                keys::STATUS
            )));
        };
        attributes.status = parsed;
    }
    attributes.owner = reader.text(keys::OWNER).unwrap_or_default();
    attributes.material = reader.text(keys::MATERIAL).unwrap_or_default();
    attributes.configuration = reader.text(keys::CONFIGURATION).unwrap_or_default();
    attributes.description = reader.text(keys::DESCRIPTION).unwrap_or_default();
    if let Some(diameter) = reader.number(keys::DIAMETER)? {
        if !(diameter > 0.0) {
            return Err(reader.refuse(&format!(
                "has a {} that is not a positive number of metres",
                keys::DIAMETER
            )));
        }
        attributes.diameter = diameter;
        attributes.diameter_is_inside = reader.flag(keys::DIAMETER_INSIDE)?;
    }
    for (key, value) in &reader.point.properties {
        if let Some(name) = key.strip_prefix(keys::FIELD_PREFIX)
            && is_line_field(name)
        {
            attributes
                .fields
                .entry(String::from(name))
                .or_insert_with(|| property_to_string(value));
        }
    }
    attributes.recorded = recorded_of(reader.point, true);
    Ok(attributes)
}

fn read_vertex(reader: &PointReader<'_>) -> Result<UtilityVertex> {
    let Some(id) = reader.text(keys::VERTEX).filter(|id| !id.is_empty()) else {
        return Err(reader.refuse(&format!("has no {}, its point id", keys::VERTEX)));
    };
    // Every caller reads a point (the C++ takes its PointGeometry unchecked).
    let Geometry::Point(point) = &reader.point.geometry else {
        return Err(reader.refuse("is not a point"));
    };
    let mut vertex = UtilityVertex {
        id,
        position: Coordinate2::new(point.position.y, point.position.x),
        ..UtilityVertex::default()
    };

    let Some(method) = reader.text(keys::METHOD) else {
        return Err(reader.refuse(&format!(
            "has no {}, the method it was located by",
            keys::METHOD
        )));
    };
    // to_string's "unknown method" is what DRAW writes for a method nobody
    // recorded; the schedule reader's word for it is "unknown".
    let parsed_method = if method == sub::to_string(LocationMethod::Unknown) {
        Some(LocationMethod::Unknown)
    } else {
        sub::parse_location_method(&method)
    };
    let Some(parsed_method) = parsed_method else {
        return Err(reader.refuse(&format!(
            "has {} \"{method}\", which is not a location method",
            keys::METHOD
        )));
    };
    vertex.evidence.method = parsed_method;

    vertex.level = reader.number(keys::LEVEL)?;
    vertex.depth = reader.number(keys::DEPTH)?;
    vertex.surface_level = reader.number(keys::SURFACE_LEVEL)?;
    vertex.evidence.horizontal_uncertainty = reader.number(keys::HORIZONTAL_UNCERTAINTY)?;
    vertex.evidence.vertical_uncertainty = reader.number(keys::VERTICAL_UNCERTAINTY)?;
    if vertex.depth.is_some_and(|depth| depth < 0.0) {
        return Err(reader.refuse(&format!(
            "has a {} above the surface; a depth is below it",
            keys::DEPTH
        )));
    }
    if let Some(reference) = reader.text(keys::LEVEL_REFERENCE) {
        let Some(parsed) = sub::parse_level_reference(&reference) else {
            return Err(reader.refuse(&format!(
                "has {} \"{reference}\", which is not a level reference",
                keys::LEVEL_REFERENCE
            )));
        };
        vertex.level_reference = parsed;
    }
    if let Some(claimed) = reader.text(keys::CLAIMED) {
        let Some(parsed) = sub::parse_quality_level(&claimed) else {
            return Err(reader.refuse(&format!(
                "has {} \"{claimed}\", which is not a quality level",
                keys::CLAIMED
            )));
        };
        vertex.claimed = Some(parsed);
    }
    vertex.verifies = reader.text(keys::VERIFIES).unwrap_or_default();
    for (key, value) in &reader.point.properties {
        let Some(name) = key.strip_prefix(keys::FIELD_PREFIX) else {
            continue;
        };
        if !is_line_field(name) {
            vertex
                .fields
                .entry(String::from(name))
                .or_insert_with(|| property_to_string(value));
        }
    }
    vertex.recorded = recorded_of(reader.point, false);
    vertex.evidence.has_level = sub::has_vertical_measurement(&vertex);
    Ok(vertex)
}

fn read_service(id: &str, points: &[&Entity]) -> Result<DrawnService> {
    // Their places along the line; a tie is broken by id only to name both.
    let mut places: Vec<(f64, &Entity)> = Vec::with_capacity(points.len());
    for &point in points {
        let reader = PointReader::new(id, point);
        let Some(order) = reader.number(keys::ORDER)? else {
            return Err(reader.refuse(&format!("has no {}, its place along the line", keys::ORDER)));
        };
        places.push((order, point));
    }
    // The places are finite (number() refuses any other), and two equal ones
    // - -0 and 0 among them - are told apart by id, as the C++'s `!=` and `<`
    // tell them.
    places.sort_by(|a, b| {
        a.0.partial_cmp(&b.0)
            .unwrap_or(Ordering::Equal)
            .then_with(|| a.1.id.cmp(&b.1.id))
    });
    let Some(&(_, first_point)) = places.first() else {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!("line {id} has no points to read it from"),
        ));
    };

    let mut service = DrawnService {
        line: UtilityLine {
            id: String::from(id),
            ..UtilityLine::default()
        },
        ..DrawnService::default()
    };
    let first = PointReader::new(id, first_point);
    let expected = line_attributes_of(first_point);
    service.line.attributes = read_attributes(&first)?;
    let metres = |key: &str| -> Result<Option<f64>> {
        let metres = first.number(key)?;
        if metres.is_some_and(|metres| !(metres >= 0.0)) {
            return Err(first.refuse(&format!(
                "has a {key} that is not a number of metres, not negative"
            )));
        }
        Ok(metres)
    };
    service.spacing = metres(keys::SPACING)?;
    service.minimum_cover = metres(keys::MINIMUM_COVER)?;

    let mut paths: Vec<Option<String>> = Vec::with_capacity(places.len());
    let mut vertex_ids: BTreeSet<String> = BTreeSet::new();
    for (i, &(order, point)) in places.iter().enumerate() {
        let reader = PointReader::new(id, point);
        if i > 0 && order == places[i - 1].0 {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                format!(
                    "line {id} has two points at {} {}: #{} and #{}",
                    keys::ORDER,
                    format_exact_real(order),
                    places[i - 1].1.id,
                    point.id
                ),
            ));
        }
        if i > 0 {
            agree(&first, &expected, &reader, &line_attributes_of(point))?;
        }
        let vertex = read_vertex(&reader)?;
        if !vertex_ids.insert(vertex.id.clone()) {
            return Err(reader.refuse("has the point id another point of the line has"));
        }
        paths.push(reader.text(keys::PATH));
        service.line.vertices.push(vertex);
        service.points.push(point.id);
    }
    // A path is to the next point, so the last point's says nothing. Any
    // other point giving one makes the list whole, the rest detected - as a
    // schedule reads its path column.
    let segments = paths.len().saturating_sub(1);
    let any_path = paths[..segments].iter().any(Option::is_some);
    if any_path {
        for (path, &(_, point)) in paths.iter().zip(&places).take(segments) {
            let mut evidence = PathEvidence::Detected;
            if let Some(path) = path {
                let Some(parsed) = sub::parse_path_evidence(path) else {
                    return Err(PointReader::new(id, point).refuse(&format!(
                        "has {} \"{path}\", which is not detected, exposed or assumed",
                        keys::PATH
                    )));
                };
                evidence = parsed;
            }
            service.line.path_evidence.push(evidence);
        }
    }
    Ok(service)
}

// "<prefix>/<word>/<leaf>" -> prefix, when the layer has that shape.
fn prefix_under(layer: &str, leaf_is: fn(&str) -> bool) -> Option<String> {
    if layer_depth(layer) < 3 || !leaf_is(layer_leaf(layer)) {
        return None;
    }
    Some(String::from(layer_parent(layer_parent(layer))))
}

fn is_points_leaf(leaf: &str) -> bool {
    leaf == "points"
}

fn is_quality_leaf(leaf: &str) -> bool {
    [
        QualityLevel::A,
        QualityLevel::B,
        QualityLevel::C,
        QualityLevel::D,
    ]
    .into_iter()
    .any(|level| leaf == sub::to_string(level))
}

fn prefix_of(model: &Model, service: &DrawnService, fallback: &str) -> String {
    for &id in &service.points {
        if let Some(entity) = model.entities.find(id)
            && let Some(prefix) = prefix_under(&entity.layer, is_points_leaf)
        {
            return prefix;
        }
    }
    for &id in &service.runs {
        if let Some(entity) = model.entities.find(id)
            && let Some(prefix) = prefix_under(&entity.layer, is_quality_leaf)
        {
            return prefix;
        }
    }
    String::from(fallback)
}

// The non-empty utility.line of `entity`, as the property panel reads it.
fn line_of(entity: &Entity) -> Option<String> {
    value_of(entity, keys::LINE)
        .map(property_to_string)
        .filter(|line| !line.is_empty())
}

/// The services any entity of `matched` belongs to (its utility.line), each
/// read whole from its points in the whole of `model`. InvalidArgument naming
/// the line, and the point by its vertex id and entity id, for: a service with
/// no points left; a point with no place along the line, or a place another
/// point of the line has; a point with no vertex id or one another point has;
/// a point with no method, or a method, level reference, claimed level or
/// path that does not read; a number that is not a finite number, a depth
/// above the surface; a point whose line attributes differ from its line's
/// first point's, or lack one it has. And, naming the point and the lines
/// whose attributes it carries, for a point that is a vertex of a schedule (it
/// has a utility.vertex or a utility.order) but no utility.line, or an empty
/// one - a line deleted by hand: in `matched`, or anywhere when it carries the
/// attributes of a line `matched` takes, which read without it would be read
/// short of a vertex.
pub fn read_utility_data(model: &Model, matched: &[EntityId]) -> Result<UtilityData> {
    // Every point and run of every line in the drawing, by line id: a line is
    // read whole, whatever part of it the scope took.
    #[derive(Default)]
    struct Members<'a> {
        points: Vec<&'a Entity>,
        runs: Vec<EntityId>,
    }
    let mut by_line: BTreeMap<String, Members<'_>> = BTreeMap::new();
    // Points that are a vertex of a schedule - they carry a point id or a
    // place along a line - but say no line: a utility.line deleted or emptied
    // by hand. Never ignored, or a line would be read short of them.
    let mut strays: Vec<&Entity> = Vec::new();
    for id in model.entities.ids() {
        let Some(entity) = model.entities.find(id) else {
            continue;
        };
        let Some(line) = line_of(entity) else {
            if is_stray_vertex(entity) {
                strays.push(entity);
            }
            continue;
        };
        let members = by_line.entry(line).or_default();
        match entity.r#type() {
            EntityType::Point => members.points.push(entity),
            // A run filleted or given an arc is stored as a curve polyline
            // (docs/drawing.md) and is still the line's run: left out, a
            // regrade would draw the run again beside it.
            EntityType::Polyline | EntityType::CurvePolyline => members.runs.push(entity.id),
            _ => {}
        }
    }
    // The lines a stray point is most likely one of: those whose points carry
    // the line attributes it carries.
    let lines_of = |stray: &Entity| -> Vec<String> {
        let attributes = line_attributes_of(stray);
        by_line
            .iter()
            .filter(|(_, members)| {
                members
                    .points
                    .iter()
                    .any(|point| line_attributes_of(point) == attributes)
            })
            .map(|(name, _)| name.clone())
            .collect()
    };

    let mut data = UtilityData::default();
    let mut points_taken: BTreeMap<String, usize> = BTreeMap::new();
    let mut wanted: BTreeSet<String> = BTreeSet::new();
    for &id in matched {
        let Some(entity) = model.entities.find(id) else {
            continue;
        };
        if strays.iter().any(|stray| stray.id == entity.id) {
            return Err(stray_refusal(entity, &lines_of(entity)));
        }
        let Some(name) = line_of(entity) else {
            data.ignored += 1;
            continue;
        };
        if entity.r#type() == EntityType::Point {
            *points_taken.entry(name.clone()).or_default() += 1;
        }
        wanted.insert(name);
    }
    // A stray outside the scope still refuses a line it may be a point of:
    // read without it, that line would be graded, reported or written short.
    for &stray in &strays {
        let lines = lines_of(stray);
        if lines.iter().any(|name| wanted.contains(name)) {
            return Err(stray_refusal(stray, &lines));
        }
    }

    let mut drawn_order: Vec<(EntityId, String)> = Vec::new();
    for name in &wanted {
        let points = by_line.get(name).map(|members| &members.points);
        let Some(first) = points.and_then(|points| points.iter().map(|point| point.id).min())
        else {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                format!(
                    "line {name} has no points left to read it from: its points are the \
                     schedule, its runs only draw it"
                ),
            ));
        };
        drawn_order.push((first, name.clone()));
    }
    drawn_order.sort();
    for (_, name) in &drawn_order {
        let Some(members) = by_line.get(name) else {
            continue;
        };
        let mut service = read_service(name, &members.points)?;
        service.runs.clone_from(&members.runs);
        service.runs.sort_unstable();
        service.completed =
            points_taken.get(name).copied().unwrap_or_default() < service.points.len();
        data.completed += usize::from(service.completed);
        data.services.push(service);
    }
    Ok(data)
}

/// The keys a UTILITY reply adds to the scope record (scope_record):
/// " lines=4 completed=1 ignored=3".
pub fn utility_data_keys(data: &UtilityData) -> String {
    format!(
        " lines={} completed={} ignored={}",
        data.services.len(),
        data.completed,
        data.ignored
    )
}

// ---- REGRADE -------------------------------------------------------------------------------

/// What UTILITY REGRADE plans. Neither Clone nor Debug: it holds the command.
#[derive(Default)]
pub struct UtilityRegrade {
    /// What the services grade as now, drawn: for the reply, whose first
    /// record carries the bounds a front end frames.
    pub drawing: UtilityDrawing,
    /// Services whose runs or points change.
    pub changed: usize,
    /// ONE step named UTILITY_REGRADE_STEP: each changed service's runs
    /// removed and drawn again, its points' graded properties (utility.*)
    /// replaced and anything else on them - a person's own properties, style,
    /// colour - left as it is, and the layers and linetypes a new run needs. A
    /// point on its type's points layer under the service's prefix follows an
    /// edited type to that type's layer; a point a person put elsewhere stays
    /// there. None when nothing would change, so no empty undo step is pushed.
    pub command: Option<CommandPtr>,
}

/// What UTILITY REGRADE was told. A setting given grades every service with
/// it; one left out grades each service with the one it was drawn with
/// (DrawnService::spacing, minimum_cover), else the default - so a regrade of
/// what nobody edited changes nothing, whatever SPACING and MINCOVER it was
/// drawn with, and a regrade after one edit changes only what the edit does.
#[derive(Clone, Debug, PartialEq)]
pub struct UtilityRegradeOptions {
    pub spacing: Option<f64>,
    pub minimum_cover: Option<f64>,
    /// The prefix a service is drawn under when neither its points' layer nor
    /// its runs' says one.
    pub layer_prefix: String,
}

impl Default for UtilityRegradeOptions {
    fn default() -> Self {
        Self {
            spacing: None,
            minimum_cover: None,
            layer_prefix: String::from(DEFAULT_UTILITY_LAYER_PREFIX),
        }
    }
}

/// The services of `data` graded again from their points with `options` (the
/// C++ default is UtilityRegradeOptions::default()). Each service is drawn
/// under the prefix it was drawn under - read from its points' layer
/// "<prefix>/<type>/points", else its runs' "<prefix>/<type>/QL-X" - and under
/// options.layer_prefix when neither says. All or nothing: a service that
/// cannot be graded refuses the whole regrade by name.
pub fn plan_utility_regrade(
    model: &Model,
    data: &UtilityData,
    options: &UtilityRegradeOptions,
) -> Result<UtilityRegrade> {
    let mut result = UtilityRegrade::default();
    let mut changes = ChangeSet::default();
    // Every layer and linetype a drawing of the services would make, by name.
    let mut definitions: BTreeMap<String, Layer> = BTreeMap::new();
    let mut linetypes: BTreeMap<String, Linetype> = BTreeMap::new();
    let mut holding: BTreeSet<String> = BTreeSet::new();

    for service in &data.services {
        // What it was drawn with, unless the regrade says otherwise.
        let mut own = UtilityDrawOptions {
            layer_prefix: prefix_of(model, service, &options.layer_prefix),
            minimum_cover: options.minimum_cover.or(service.minimum_cover),
            ..UtilityDrawOptions::default()
        };
        if let Some(spacing) = options.spacing.or(service.spacing) {
            own.grading.maximum_detected_spacing = spacing;
        }
        let drawn = draw_utilities(std::slice::from_ref(&service.line), &own)?;
        for layer in drawn.layers {
            definitions.entry(layer.name.clone()).or_insert(layer);
        }
        for linetype in drawn.linetypes {
            linetypes.entry(linetype.name.clone()).or_insert(linetype);
        }

        let (points, runs): (Vec<&Entity>, Vec<&Entity>) = drawn
            .entities
            .iter()
            .partition(|entity| entity.r#type() == EntityType::Point);
        // The points keep what is theirs and take what the grading says now.
        let mut changed = false;
        for (point, &id) in points.iter().zip(&service.points) {
            let Some(old) = model.entities.find(id) else {
                continue;
            };
            let mut updated = old.clone();
            updated.properties.retain(|key, _| !is_drawn_key(key));
            for (key, value) in &point.properties {
                updated.properties.insert(key.clone(), value.clone());
            }
            if prefix_under(&old.layer, is_points_leaf).as_deref()
                == Some(own.layer_prefix.as_str())
            {
                updated.layer.clone_from(&point.layer);
            }
            holding.insert(updated.layer.clone());
            if updated != *old {
                changes.modify.push(updated);
                changed = true;
            }
        }
        // The runs are drawn again only when they differ, so a regrade of a
        // line nobody edited leaves its entities, and their ids, alone.
        let mut same_runs = runs.len() == service.runs.len();
        for (run, &id) in runs.iter().zip(&service.runs) {
            if !same_runs {
                break;
            }
            same_runs = model.entities.find(id).is_some_and(|old| {
                old.geometry == run.geometry
                    && old.layer == run.layer
                    && old.properties == run.properties
            });
        }
        for run in &runs {
            holding.insert(run.layer.clone());
        }
        if !same_runs {
            changes.remove.extend_from_slice(&service.runs);
            for &run in &runs {
                changes.add.push(run.clone());
            }
            changed = true;
        }
        result.changed += usize::from(changed);

        let total = &mut result.drawing;
        total.lines.extend(drawn.lines.into_iter().take(1));
        total.vertices += drawn.vertices;
        total.segments += drawn.segments;
        total.bounds.expand_box(drawn.bounds);
        total.entities.extend(drawn.entities);
    }
    result.drawing.drawn_layers = holding.len();
    result.drawing.layers = definitions.values().cloned().collect();
    result.drawing.linetypes = linetypes.values().cloned().collect();
    if changes.empty() {
        return Ok(result);
    }

    // The layers what is added or moved lands on that the model lacks, with
    // their ancestors, parents first - a set of paths sorts a parent before
    // its children - and the linetypes those new layers name.
    let mut make: BTreeSet<String> = BTreeSet::new();
    let mut need = |layer: &str| {
        if !model.layers.contains(layer) && definitions.contains_key(layer) {
            make.insert(String::from(layer));
            for ancestor in layer_ancestors(layer) {
                if !model.layers.contains(&ancestor) && definitions.contains_key(&ancestor) {
                    make.insert(ancestor);
                }
            }
        }
    };
    for entity in changes.add.iter().chain(&changes.modify) {
        need(&entity.layer);
    }
    let mut transaction = Transaction::new(UTILITY_REGRADE_STEP);
    let named: BTreeSet<&str> = make
        .iter()
        .filter_map(|layer| definitions.get(layer))
        .map(|layer| layer.linetype.as_str())
        .collect();
    for (name, linetype) in &linetypes {
        if named.contains(name.as_str()) && !model.linetypes.contains(name) {
            transaction.add(create_linetype(linetype.clone()));
        }
    }
    for layer in &make {
        if let Some(definition) = definitions.get(layer) {
            transaction.add(create_layer(definition.clone()));
        }
    }
    transaction.add(Box::new(ChangeSetCommand::new(
        "UTILITY_REGRADE_ENTITIES",
        move |_: &CommandContext<'_>| Ok(changes.clone()),
        false,
    )));
    result.command = Some(Box::new(transaction));
    Ok(result)
}

// ---- services from surveyed or imported geometry -----------------------------------------
//
// What UTILITY DRAW <scope> reads (docs/subsurface_utilities.md, "Services from
// surveyed and imported geometry"): the located services as a survey or an
// import left them in the drawing - a coded survey's strings, a .12da
// archive's, a DXF's, a shapefile's or an IFC file's lines - rather than as a
// schedule file. Each line or open polyline in scope is one service, its
// vertices the located vertices in order; a point in scope on a vertex gives
// that vertex its point number and anything it says of itself. What the
// geometry cannot say - how it was located, to what uncertainty, what kind of
// service - comes from utility.* properties on the entities (keys,
// utility_drawing.rs: set by MODIFY ... SET PROP=, the property panel, or an
// import's own attributes of those names) and, for what they leave unsaid,
// from the verb's options.

/// What the heights of the geometry are the heights of.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum GeometryHeights {
    /// The ground over the service: a detection marked on the surface and shot
    /// there, which is how most located services are surveyed. The default,
    /// because it errs the safe way: a service level read as the surface leaves
    /// the vertex without a level - QL-B at best, cover not computed - where a
    /// surface read as the service would claim a measured level at ground and
    /// a cover of nothing.
    #[default]
    Surface,
    /// The service itself, on its level reference: shot on the pipe in a
    /// pothole, or a design or as-constructed model's centre line.
    Service,
    /// Neither: the heights are not to be used.
    Unused,
}

/// "surface", "service", "none": the word HEIGHTS and keys::HEIGHTS take.
pub fn geometry_heights_word(heights: GeometryHeights) -> &'static str {
    match heights {
        GeometryHeights::Surface => "surface",
        GeometryHeights::Service => "service",
        GeometryHeights::Unused => "none",
    }
}

/// Those words in any case; None for anything else.
pub fn parse_geometry_heights(text: &str) -> Option<GeometryHeights> {
    [
        GeometryHeights::Surface,
        GeometryHeights::Service,
        GeometryHeights::Unused,
    ]
    .into_iter()
    .find(|&heights| {
        equals_ignoring_case(
            trimmed_text(text).as_bytes(),
            geometry_heights_word(heights).as_bytes(),
        )
    })
}

/// What UTILITY DRAW <scope> says for every service and vertex that does not
/// say it itself. Nothing given is nothing assumed: no method refuses the
/// vertex that has none, no uncertainty is "not assessed", no type is Unknown.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GeometryServiceOptions {
    pub r#type: Option<UtilityType>,
    pub method: Option<LocationMethod>,
    pub horizontal_uncertainty: Option<f64>,
    pub vertical_uncertainty: Option<f64>,
    pub level_reference: Option<LevelReference>,
    /// What is known between each vertex and the next.
    pub path: Option<PathEvidence>,
    pub owner: String,
    pub material: String,
    /// Metres.
    pub diameter: Option<f64>,
    pub status: Option<UtilityStatus>,
    pub heights: GeometryHeights,
    /// An import's own attributes read as schedule columns, (column,
    /// property): ("type", "ASSET_TYPE") reads an entity's ASSET_TYPE as its
    /// type, as a schedule's type column is read. The column by its schedule
    /// name or an alias (utility_csv_column_named); "line" and "point" name
    /// the service and the vertex. The entity's own utility.* property, where
    /// it has one, wins over a field read so: it is what a person set here.
    pub fields: Vec<(String, String)>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct GeometryServices {
    /// In the order drawn: by the id of the entity each was read from.
    pub lines: Vec<UtilityLine>,
    /// lines[i] was read from sources[i].
    pub sources: Vec<EntityId>,
    /// Points in scope that gave a vertex its point number and values.
    pub points: usize,
    /// Points in scope on no vertex of a line taken: not a service on their
    /// own.
    pub loose: usize,
    /// In scope and drawn already: a drawn service's run or point, or an
    /// entity a service was drawn from before (keys::SOURCE).
    pub drawn: usize,
    /// What cannot be a service: a closed polyline (an outline - a pit, a
    /// building, a parcel - not a run), an arc, a circle, a text, a label ...
    pub ignored: usize,
}

// What a line entity may say of the service it is.
const SERVICE_KEYS: [&str; 8] = [
    keys::TYPE,
    keys::OWNER,
    keys::MATERIAL,
    keys::DIAMETER,
    keys::DIAMETER_INSIDE,
    keys::CONFIGURATION,
    keys::DESCRIPTION,
    keys::STATUS,
];
// What it may say of every vertex of it.
const LINE_VERTEX_KEYS: [&str; 7] = [
    keys::METHOD,
    keys::HORIZONTAL_UNCERTAINTY,
    keys::VERTICAL_UNCERTAINTY,
    keys::LEVEL_REFERENCE,
    keys::CLAIMED,
    keys::PATH,
    keys::DEPTH,
];
// What a point on a vertex may say of that vertex: its whole row but the
// line's attributes, which are the line's to say.
const POINT_VERTEX_KEYS: [&str; 10] = [
    keys::METHOD,
    keys::LEVEL,
    keys::DEPTH,
    keys::SURFACE_LEVEL,
    keys::LEVEL_REFERENCE,
    keys::HORIZONTAL_UNCERTAINTY,
    keys::VERTICAL_UNCERTAINTY,
    keys::CLAIMED,
    keys::VERIFIES,
    keys::PATH,
];

// The key a schedule column is drawn as (utility_drawing.rs, keys), for a
// field FIELDS reads as that column. Empty for "line" and "point", which are
// the ids, not properties.
const KEY_OF: [(&str, &str); 19] = [
    ("line", ""),
    ("point", ""),
    ("method", keys::METHOD),
    ("level", keys::LEVEL),
    ("level_ref", keys::LEVEL_REFERENCE),
    ("depth", keys::DEPTH),
    ("surface", keys::SURFACE_LEVEL),
    ("h_unc", keys::HORIZONTAL_UNCERTAINTY),
    ("v_unc", keys::VERTICAL_UNCERTAINTY),
    ("ql", keys::CLAIMED),
    ("path", keys::PATH),
    ("verifies", keys::VERIFIES),
    ("type", keys::TYPE),
    ("owner", keys::OWNER),
    ("material", keys::MATERIAL),
    ("diameter_mm", keys::DIAMETER),
    ("status", keys::STATUS),
    ("config", keys::CONFIGURATION),
    ("description", keys::DESCRIPTION),
];

struct FieldColumn {
    // The schedule's name for it.
    column: String,
    key: String,
    // The entity's property it is read from.
    property: String,
}

fn field_columns(fields: &[(String, String)]) -> Result<Vec<FieldColumn>> {
    let mut out: Vec<FieldColumn> = Vec::new();
    for (name, property) in fields {
        let Some(column) = sub::utility_csv_column_named(name) else {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                format!("FIELDS: {name} is not a column of a utility schedule"),
            ));
        };
        let key = if column.carried != CarriedOn::Interpreted {
            format!("{}{}", keys::FIELD_PREFIX, column.name)
        } else if let Some(&(_, key)) = KEY_OF.iter().find(|(named, _)| *named == column.name) {
            String::from(key)
        } else {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                format!(
                    "FIELDS: {}{}",
                    column.name,
                    if column.name == "size" {
                        " is read from a schedule only; give the diameter as diameter_mm"
                    } else {
                        " is where the geometry is, not a field"
                    }
                ),
            ));
        };
        if property.is_empty() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                format!("FIELDS: {} names no property", column.name),
            ));
        }
        if out.iter().any(|taken| taken.column == column.name) {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                format!("FIELDS: {} given twice", column.name),
            ));
        }
        out.push(FieldColumn {
            column: String::from(column.name),
            key,
            property: property.clone(),
        });
    }
    Ok(out)
}

// `entity` as the reader reads it: its fields under the keys of their columns,
// where it has no utility.* property of its own there, and its ids in `line`
// and `point` when a field names them.
struct EntityView {
    entity: Entity,
    line: String,
    point: String,
}

fn view_of(entity: &Entity, fields: &[FieldColumn]) -> EntityView {
    let mut view = EntityView {
        entity: entity.clone(),
        line: String::new(),
        point: String::new(),
    };
    for field in fields {
        let Some(value) = value_of(entity, &field.property) else {
            continue;
        };
        let text = String::from(trimmed_text(&property_to_string(value)));
        if field.column == "line" {
            view.line = text;
            continue;
        }
        if field.column == "point" {
            view.point = text;
            continue;
        }
        if text.is_empty() || view.entity.properties.contains_key(&field.key) {
            continue;
        }
        let mut read = value.clone();
        // A schedule's diameter is millimetres; the drawing's is metres.
        if field.column == "diameter_mm"
            && let Some(millimetres) = parse_finite_double(&text)
        {
            read = PropertyValue::Real(millimetres / 1000.0);
        }
        view.entity.properties.insert(field.key.clone(), read);
    }
    view
}

fn has_line(entity: &Entity) -> bool {
    line_of(entity).is_some()
}

// The entity's name: its code, as survey coding finds one, a coded survey's
// string name or a .12da archive string's. Empty when it has none.
fn name_of(entity: &Entity) -> String {
    for candidate in code_property_candidates() {
        if let Some(code) = survey_code_of(entity, candidate) {
            let name = trimmed_text(code);
            if !name.is_empty() {
                return String::from(name);
            }
        }
    }
    String::new()
}

// "polyline (#12)", "line (#7)": the entity a service is read from, as a
// refusal names it.
fn source_label(entity: &Entity) -> String {
    format!(
        "{} (#{})",
        entity.r#type().name().to_ascii_lowercase(),
        entity.id
    )
}

// The vertices of a line entity that can be a service: a line, or an open
// polyline. Empty for anything else.
fn run_vertices(entity: &Entity) -> Vec<Point2> {
    match &entity.geometry {
        Geometry::Line(segment) => vec![segment.start, segment.end],
        Geometry::Polyline(polyline) if !polyline.closed => polyline.vertices.clone(),
        _ => Vec::new(),
    }
}

fn copy_keys(from: &Entity, keys_to_copy: &[&str], into: &mut PropertyMap) {
    for &key in keys_to_copy {
        if let Some(value) = value_of(from, key) {
            into.insert(String::from(key), value.clone());
        }
    }
}

// A delivery schema's fields on `from`: the line's (`line`) or a vertex's.
fn copy_fields(from: &Entity, line: bool, into: &mut PropertyMap) {
    for (key, value) in &from.properties {
        if key
            .strip_prefix(keys::FIELD_PREFIX)
            .is_some_and(|name| is_line_field(name) == line)
        {
            into.insert(key.clone(), value.clone());
        }
    }
}

// What the verb's options say, as the properties a drawn point would carry.
fn service_defaults(options: &GeometryServiceOptions) -> PropertyMap {
    let mut properties = PropertyMap::new();
    let mut text = |key: &str, value: &str| {
        if !value.is_empty() {
            properties.insert(String::from(key), text_value(value));
        }
    };
    if let Some(r#type) = options.r#type {
        text(keys::TYPE, utility_type_word(r#type));
    }
    text(keys::OWNER, &options.owner);
    text(keys::MATERIAL, &options.material);
    if let Some(status) = options.status {
        text(keys::STATUS, sub::to_string(status));
    }
    // After the texts, where the C++ inserts it before the status: the keys
    // differ, so the map is the same either way.
    if let Some(diameter) = options.diameter {
        properties.insert(String::from(keys::DIAMETER), PropertyValue::Real(diameter));
    }
    properties
}

fn vertex_defaults(options: &GeometryServiceOptions) -> PropertyMap {
    let mut properties = PropertyMap::new();
    if let Some(method) = options.method {
        properties.insert(
            String::from(keys::METHOD),
            text_value(sub::to_string(method)),
        );
    }
    if let Some(uncertainty) = options.horizontal_uncertainty {
        properties.insert(
            String::from(keys::HORIZONTAL_UNCERTAINTY),
            PropertyValue::Real(uncertainty),
        );
    }
    if let Some(uncertainty) = options.vertical_uncertainty {
        properties.insert(
            String::from(keys::VERTICAL_UNCERTAINTY),
            PropertyValue::Real(uncertainty),
        );
    }
    if let Some(reference) = options.level_reference {
        properties.insert(
            String::from(keys::LEVEL_REFERENCE),
            text_value(sub::to_string(reference)),
        );
    }
    if let Some(path) = options.path {
        properties.insert(String::from(keys::PATH), text_value(sub::to_string(path)));
    }
    properties
}

// What an entity's keys::HEIGHTS says, refused by `reader` when it says
// something else.
fn heights_said_by(reader: &PointReader<'_>) -> Result<Option<GeometryHeights>> {
    let Some(word) = reader.text(keys::HEIGHTS) else {
        return Ok(None);
    };
    match parse_geometry_heights(&word) {
        Some(heights) => Ok(Some(heights)),
        None => Err(reader.refuse(&format!(
            "has {} \"{word}\", which is not surface, service or none",
            keys::HEIGHTS
        ))),
    }
}

// What the line entity says for the service and each of its vertices, read as
// a drawn point's would be so that a value that does not read is refused by
// the entity it is on, before a point of the service names it.
fn check_line_says(
    line: &str,
    source: &Entity,
    service: &PropertyMap,
    vertex: &PropertyMap,
) -> Status {
    let mut probe = Entity {
        id: source.id,
        geometry: Geometry::Point(PointGeometry::default()),
        properties: service.clone(),
        ..Entity::default()
    };
    for (key, value) in vertex {
        probe.properties.insert(key.clone(), value.clone());
    }
    // What only a vertex has, so that read_vertex reads the rest.
    probe
        .properties
        .insert(String::from(keys::VERTEX), text_value("-"));
    probe
        .properties
        .entry(String::from(keys::METHOD))
        .or_insert_with(|| text_value(sub::to_string(LocationMethod::Records)));
    let reader = PointReader::labelled(line, &probe, source_label(source));
    read_attributes(&reader)?;
    read_vertex(&reader)?;
    if let Some(path) = reader.text(keys::PATH)
        && sub::parse_path_evidence(&path).is_none()
    {
        return Err(reader.refuse(&format!(
            "has {} \"{path}\", which is not detected, exposed or assumed",
            keys::PATH
        )));
    }
    Ok(())
}

fn view_position(view: &EntityView) -> Point2 {
    match &view.entity.geometry {
        Geometry::Point(point) => point.position,
        _ => Point2::default(),
    }
}

// Two marks this close are one place: the survey's coordinate tolerance.
const SAME_MARK: f64 = tolerance::COORDINATE;

// The points on a place, for a vertex: `points` sorted by x, a window each.
fn points_on<'a>(points: &[&'a EntityView], place: Point2) -> Vec<&'a EntityView> {
    let start = points.partition_point(|point| view_position(point).x < place.x - SAME_MARK);
    points[start..]
        .iter()
        .take_while(|point| view_position(point).x <= place.x + SAME_MARK)
        .filter(|point| distance(view_position(point), place) < SAME_MARK)
        .copied()
        .collect()
}

/// The services the lines and open polylines of `matched` are, each read whole
/// from the entity; the points of `matched` supply their vertices (the C++
/// default options are GeometryServiceOptions::default()).
///
/// A service's id is the entity's name - a field that names it (FIELDS line=),
/// else the first code or name code_property_candidates() finds on it
/// (survey_coding.rs), a coded survey's string name or a .12da archive
/// string's - else "#<entity id>"; a name two entities in scope share, or a
/// service drawn already has, is told apart as "<name>#<entity id>". A
/// vertex's id is the point number (FIELDS point=, else the survey import's
/// "point" property) of the point on it, else "<line id>-<n>".
///
/// Each value is the most particular said: the point's own utility.* property,
/// then the line entity's, then `options`; and a height - the line's at the
/// vertex, else its point's - is a surface level or a service level as the
/// point's keys::HEIGHTS, else the line's, else options.heights says, where the
/// point does not give that level itself. The line's attributes (type, owner,
/// material, diameter, status, configuration, description, and a delivery
/// schema's line fields) come from the line entity and `options`, never from a
/// point.
///
/// A line entity's depth (keys::DEPTH, or a depth field) is every vertex's: a
/// record of a main laid at 0.9 m says so of all of it.
///
/// InvalidArgument for a field whose column the schedule does not know, or
/// which is where the geometry already is (easting, northing) or is read only
/// from a schedule (size); and, naming the line and the entity: two points on
/// one vertex (within tolerance::COORDINATE); a vertex no point, line or
/// option gives a method; a value on an entity that does not read, as
/// read_utility_data would refuse it on a drawn point; a vertex id twice in
/// one line; a point that is a vertex of a schedule but has no line
/// (read_utility_data's stray point).
pub fn read_geometry_services(
    model: &Model,
    matched: &[EntityId],
    options: &GeometryServiceOptions,
) -> Result<GeometryServices> {
    // Every line drawn, and every entity a line was drawn from.
    let mut lines_drawn: BTreeSet<String> = BTreeSet::new();
    let mut sources_drawn: BTreeSet<String> = BTreeSet::new();
    model.entities.for_each(|entity| {
        let Some(line) = line_of(entity) else {
            return;
        };
        lines_drawn.insert(line);
        if let Some(source) = value_of(entity, keys::SOURCE) {
            sources_drawn.insert(property_to_string(source));
        }
    });

    let fields = field_columns(&options.fields)?;
    let mut out = GeometryServices::default();
    // Each entity taken, as read through the fields, by id; the lists below
    // name them, and are made into references once every one is read.
    let mut views: BTreeMap<EntityId, EntityView> = BTreeMap::new();
    // To be drawn.
    let mut run_ids: Vec<EntityId> = Vec::new();
    // Drawn from before: their points are not loose.
    let mut before: Vec<&Entity> = Vec::new();
    // That may be on a vertex.
    let mut point_ids: Vec<EntityId> = Vec::new();
    for &id in matched {
        let Some(entity) = model.entities.find(id) else {
            continue;
        };
        if views.contains_key(&id) {
            continue;
        }
        if has_line(entity) {
            out.drawn += 1;
            continue;
        }
        if is_stray_vertex(entity) {
            return Err(stray_refusal(entity, &[]));
        }
        if entity.r#type() == EntityType::Point {
            views.insert(id, view_of(entity, &fields));
            point_ids.push(id);
        } else if run_vertices(entity).len() < 2 {
            out.ignored += 1;
        } else if sources_drawn.contains(&format!("#{id}")) {
            out.drawn += 1;
            before.push(entity);
        } else {
            views.insert(id, view_of(entity, &fields));
            run_ids.push(id);
        }
    }
    let mut runs: Vec<&EntityView> = run_ids.iter().filter_map(|id| views.get(id)).collect();
    runs.sort_by_key(|view| view.entity.id);

    // The points on a place, for each vertex: sorted by x, a window each.
    let mut points: Vec<&EntityView> = point_ids.iter().filter_map(|id| views.get(id)).collect();
    points.sort_by(|a, b| {
        view_position(a)
            .x
            .partial_cmp(&view_position(b).x)
            .unwrap_or(Ordering::Equal)
            .then_with(|| a.entity.id.cmp(&b.entity.id))
    });
    // A run's name: the field that names it, else its code.
    let name_of_run = |run: &EntityView| {
        if run.line.is_empty() {
            name_of(&run.entity)
        } else {
            run.line.clone()
        }
    };

    // Each run's id: its name, told apart from another of the same name.
    let mut named: BTreeMap<String, usize> = BTreeMap::new();
    for &view in &runs {
        let name = name_of_run(view);
        if !name.is_empty() {
            *named.entry(name).or_default() += 1;
        }
    }
    let point_number = SurveyImportOptions::default().point_number_property;
    let mut ids: BTreeSet<String> = BTreeSet::new();
    let mut used: BTreeSet<EntityId> = BTreeSet::new();
    for &view in &runs {
        let run = &view.entity;
        let name = name_of_run(view);
        let own = format!("#{}", run.id);
        let id = if name.is_empty() {
            own
        } else if named.get(&name).copied().unwrap_or_default() > 1 || lines_drawn.contains(&name) {
            format!("{name}{own}")
        } else {
            name
        };
        if lines_drawn.contains(&id) || !ids.insert(id.clone()) {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                format!(
                    "the service read from {} would be line {id}, which is {}; name it apart \
                     (its code)",
                    source_label(run),
                    if lines_drawn.contains(&id) {
                        "drawn already"
                    } else {
                        "another's too"
                    }
                ),
            ));
        }

        let mut service = service_defaults(options);
        copy_keys(run, &SERVICE_KEYS, &mut service);
        copy_fields(run, true, &mut service);
        service
            .entry(String::from(keys::TYPE))
            .or_insert_with(|| text_value(utility_type_word(UtilityType::Unknown)));
        let mut line_vertex = vertex_defaults(options);
        copy_keys(run, &LINE_VERTEX_KEYS, &mut line_vertex);
        check_line_says(&id, run, &service, &line_vertex)?;
        let line_heights = heights_said_by(&PointReader::labelled(&id, run, source_label(run)))?;

        let vertices = run_vertices(run);
        let heights = heights_of(&run.properties, vertices.len());
        let mut row: Vec<Entity> = Vec::with_capacity(vertices.len());
        for (i, &place) in vertices.iter().enumerate() {
            let on = points_on(&points, place);
            if on.len() > 1 {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    format!(
                        "line {id}: vertex {} of {} has two points on it, #{} and #{}; take the \
                         one that is not the survey's out of the scope",
                        i + 1,
                        source_label(run),
                        on[0].entity.id,
                        on[1].entity.id
                    ),
                ));
            }
            let on_vertex = on.first().copied();
            let point = on_vertex.map(|on_vertex| &on_vertex.entity);
            let mut vertex = Entity {
                id: point.map_or(run.id, |point| point.id),
                geometry: Geometry::Point(PointGeometry { position: place }),
                properties: service.clone(),
                ..Entity::default()
            };
            for (key, value) in &line_vertex {
                vertex.properties.insert(key.clone(), value.clone());
            }

            // The height, and what it is the height of.
            let mut height = heights.get(i).copied().flatten();
            let mut meaning = line_heights;
            if let Some(point) = point {
                if height.is_none() {
                    height = heights_of(&point.properties, 1).first().copied().flatten();
                }
                let point_heights = heights_said_by(&PointReader::new(&id, point))?;
                meaning = point_heights.or(meaning);
            }
            if let Some(height) = height {
                match meaning.unwrap_or(options.heights) {
                    GeometryHeights::Surface => {
                        vertex.properties.insert(
                            String::from(keys::SURFACE_LEVEL),
                            PropertyValue::Real(height),
                        );
                    }
                    GeometryHeights::Service => {
                        vertex
                            .properties
                            .insert(String::from(keys::LEVEL), PropertyValue::Real(height));
                    }
                    GeometryHeights::Unused => {}
                }
            }

            let mut vertex_id = format!("{id}-{}", i + 1);
            if let (Some(point), Some(on_vertex)) = (point, on_vertex) {
                copy_keys(point, &POINT_VERTEX_KEYS, &mut vertex.properties);
                copy_fields(point, false, &mut vertex.properties);
                if !on_vertex.point.is_empty() {
                    vertex_id.clone_from(&on_vertex.point);
                } else if let Some(number) = value_of(point, &point_number) {
                    let text = property_to_string(number);
                    let trimmed = trimmed_text(&text);
                    if !trimmed.is_empty() {
                        vertex_id = String::from(trimmed);
                    }
                }
                used.insert(point.id);
            }
            vertex
                .properties
                .insert(String::from(keys::LINE), text_value(&id));
            vertex
                .properties
                .insert(String::from(keys::VERTEX), text_value(&vertex_id));
            vertex.properties.insert(
                String::from(keys::ORDER),
                PropertyValue::Integer((i + 1) as i64),
            );
            if !vertex.properties.contains_key(keys::METHOD) {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    format!(
                        "line {id}: vertex {vertex_id}{} says no method it was located by; give \
                         METHOD, or {} on its point or on {}",
                        point
                            .map(|point| format!(" (#{})", point.id))
                            .unwrap_or_default(),
                        keys::METHOD,
                        source_label(run)
                    ),
                ));
            }
            row.push(vertex);
        }
        let members: Vec<&Entity> = row.iter().collect();
        let read = read_service(&id, &members)?;
        out.lines.push(read.line);
        out.sources.push(run.id);
    }

    // A point of a service drawn before is that service's, not loose.
    let mut drawn_points: BTreeSet<EntityId> = BTreeSet::new();
    for run in &before {
        for vertex in run_vertices(run) {
            for point in points_on(&points, vertex) {
                if !used.contains(&point.entity.id) {
                    drawn_points.insert(point.entity.id);
                }
            }
        }
    }
    out.points = used.len();
    out.drawn += drawn_points.len();
    out.loose = points.len() - used.len() - drawn_points.len();
    Ok(out)
}

/// The keys a UTILITY DRAW <scope> reply adds to the scope record
/// (scope_record): " lines=2 points=9 loose=1 drawn=0 ignored=3".
pub fn geometry_service_keys(services: &GeometryServices) -> String {
    format!(
        " lines={} points={} loose={} drawn={} ignored={}",
        services.lines.len(),
        services.points,
        services.loose,
        services.drawn,
        services.ignored
    )
}

/// The drawing of `services`, as draw_utilities draws a schedule, each point
/// carrying the entity its line was read from (keys::SOURCE).
pub fn draw_geometry_services(
    services: &GeometryServices,
    options: &UtilityDrawOptions,
) -> Result<UtilityDrawing> {
    let mut drawing = draw_utilities(&services.lines, options)?;
    let mut source_of: BTreeMap<&str, String> = BTreeMap::new();
    for (line, source) in services.lines.iter().zip(&services.sources) {
        source_of
            .entry(line.id.as_str())
            .or_insert_with(|| format!("#{source}"));
    }
    for entity in &mut drawing.entities {
        if entity.r#type() != EntityType::Point {
            continue;
        }
        let Some(line) = value_of(entity, keys::LINE).map(property_to_string) else {
            continue;
        };
        if let Some(found) = source_of.get(line.as_str()) {
            entity
                .properties
                .insert(String::from(keys::SOURCE), text_value(found));
        }
    }
    Ok(drawing)
}

// ---- the design CLEARANCE is measured against ----------------------------------------------

/// Within a millimetre: the clearance report's figures are to the millimetre.
pub const DESIGN_CHORD_TOLERANCE: f64 = 0.001;

/// Proposed works drawn as an entity: a line, a polyline (a closed one returns
/// to its start) or a curve polyline (its arcs as chords within a millimetre).
/// Levels: `level` at every vertex when given; else the entity's own heights
/// (heights_of, or a curve polyline's vertices, interpolated along a
/// segment), where it has them; else none. The id is "#<entity id>".
/// InvalidArgument for any other kind of entity.
pub fn design_from_entity(entity: &Entity, level: Option<f64>) -> Result<DesignAlignment> {
    let fewer = || {
        Error::new(
            ErrorCode::InvalidArgument,
            format!("entity #{} has fewer than two vertices", entity.id),
        )
    };
    // The vertices the entity itself has, before a closed polyline's return.
    let (points, own) = match &entity.geometry {
        Geometry::Line(segment) => (vec![segment.start, segment.end], 2),
        Geometry::Polyline(polyline) => {
            let mut points = polyline.vertices.clone();
            let own = points.len();
            if polyline.closed && points.len() >= 2 {
                let start = points[0];
                points.push(start);
            }
            (points, own)
        }
        Geometry::CurvePolyline(curve) => {
            // A centre line with arcs - drawn so, or read from DXF with
            // bulges - as chords within a millimetre, each with the height its
            // vertices give it, interpolated along a segment.
            let design = DesignAlignment {
                id: format!("#{}", entity.id),
                vertices: curve
                    .tessellate_with_heights(CURVE_CHORD_TOLERANCE)
                    .into_iter()
                    .map(|point| DesignVertex {
                        position: Coordinate2::new(point.position.y, point.position.x),
                        level: level.or(point.height),
                    })
                    .collect(),
                ..DesignAlignment::default()
            };
            if design.vertices.len() < 2 {
                return Err(fewer());
            }
            return Ok(design);
        }
        _ => {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                format!(
                    "entity #{} is {}; the design centre line is a line, a polyline or a curve \
                     polyline",
                    entity.id,
                    entity.r#type().name().to_ascii_lowercase()
                ),
            ));
        }
    };
    if points.len() < 2 {
        return Err(fewer());
    }
    let mut heights: Vec<Option<f64>> = if level.is_some() {
        vec![level; own]
    } else {
        heights_of(&entity.properties, own)
    };
    if heights.len() < points.len() {
        // The closing vertex is the first.
        let start = heights.first().copied().flatten();
        heights.push(start);
    }
    Ok(DesignAlignment {
        id: format!("#{}", entity.id),
        vertices: points
            .iter()
            .zip(&heights)
            .map(|(point, &height)| DesignVertex {
                position: Coordinate2::new(point.y, point.x),
                level: height,
            })
            .collect(),
        ..DesignAlignment::default()
    })
}

// An alignment solved, and the stations its design is sampled at.
struct SampledAlignment {
    solved: SolvedAlignment,
    profile: Option<SolvedProfile>,
    stations: Vec<f64>,
}

fn divide(stations: &mut Vec<f64>, start: f64, length: f64, pieces: usize) {
    let pieces = pieces.max(1);
    for k in 0..=pieces {
        stations.push(start + length * k as f64 / pieces as f64);
    }
}

fn sample_alignment(alignment: &Alignment, tolerance: f64) -> Result<SampledAlignment> {
    let solved = katana_geometry::alignment::solve_alignment(&alignment.horizontal)?;
    let profile = alignment.vertical.as_ref().map(solve_profile).transpose()?;

    // Where the plan geometry needs a vertex: each element's ends, and along a
    // curve as often as the one sagitta rule asks.
    let mut stations: Vec<f64> = Vec::new();
    for element in solved.elements() {
        let pieces = match &element.shape {
            AlignmentShape::Arc(arc) => sagitta_chord_count(arc.radius, arc.sweep, tolerance),
            AlignmentShape::Spiral(spiral) => spiral.chord_count_for(tolerance) - 1,
            AlignmentShape::Segment(_) => 1,
        };
        divide(&mut stations, element.start_station, element.length, pieces);
    }
    // Where the levels need one: each key station of the profile, and inside a
    // vertical curve closely enough that the straight line between two samples
    // stays within `tolerance` of the parabola. A chord of length l on a
    // parabola whose grade changes by A over L departs from it by at most
    // A l^2 / (8 L), at its middle, so l = sqrt(8 L tolerance / A). Capped at
    // 8192 pieces, as sagitta_chord_count caps a circle's.
    if let Some(profile) = &profile {
        stations.extend(profile.key_stations());
        for element in profile.elements() {
            let change = (element.end_grade - element.start_grade).abs();
            if element.kind != ProfileElementKind::Curve
                || !(change > 0.0)
                || !(element.length > 0.0)
            {
                continue;
            }
            let chord = (8.0 * element.length * tolerance / change).sqrt();
            let pieces = cpp::min((element.length / chord).ceil(), 8192.0);
            divide(
                &mut stations,
                element.start_station,
                element.length,
                pieces as usize,
            );
        }
    }
    // The ends are the alignment's own, and every station is held inside them
    // rather than dropped: the last element's end is the elements' lengths
    // summed from the start, which can come out one rounding past
    // end_station() - and dropping it would drop the whole last tangent. A
    // profile station off either end folds onto that end, and the dedup below
    // makes it one with it.
    let first = solved.start_station();
    let last = solved.end_station();
    for station in &mut stations {
        *station = cpp::clamp(*station, first, last);
    }
    stations.push(first);
    stations.push(last);
    stations.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    // Stations a rounding apart are one: a key station of the profile falls on
    // an element's end as often as not.
    stations.dedup_by(|next, kept| (*kept - *next).abs() < 1e-9);
    // A station within that of an end is the end itself.
    if let Some(front) = stations.first_mut() {
        *front = first;
    }
    if let Some(back) = stations.last_mut() {
        *back = last;
    }
    Ok(SampledAlignment {
        solved,
        profile,
        stations,
    })
}

/// The stations design_from_alignment samples, ascending, one per vertex: for
/// a test to hold each chord to the curve it stands for (DESIGN_CHORD_TOLERANCE
/// is the C++ default tolerance).
pub fn design_stations(alignment: &Alignment, tolerance: f64) -> Result<Vec<f64>> {
    Ok(sample_alignment(alignment, tolerance)?.stations)
}

/// Proposed works along a document alignment: its horizontal geometry,
/// chorded to `tolerance` (sagitta_chord_count's rule), and levels from its
/// design profile where it has one - sampled at every key station of the
/// profile and closely enough inside each vertical curve that the straight
/// lines between samples stay within `tolerance` of the parabola. Stations the
/// profile does not reach have no level. The id is the alignment's name.
/// Fails with whatever solve_alignment or solve_profile reports
/// (DESIGN_CHORD_TOLERANCE is the C++ default tolerance).
pub fn design_from_alignment(alignment: &Alignment, tolerance: f64) -> Result<DesignAlignment> {
    let sampled = sample_alignment(alignment, tolerance)?;
    let mut design = DesignAlignment {
        id: alignment.name.clone(),
        ..DesignAlignment::default()
    };
    for &station in &sampled.stations {
        let Some(point) = sampled.solved.point_at_station(station) else {
            continue;
        };
        design.vertices.push(DesignVertex {
            position: Coordinate2::new(point.y, point.x),
            level: sampled
                .profile
                .as_ref()
                .and_then(|profile| profile.elevation_at(station)),
        });
    }
    Ok(design)
}
