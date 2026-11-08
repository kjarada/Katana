//! The survey tools of the Survey menu and the command line: inverse, forward
//! (radiate), area, traverse, level book, the angle calculator and the
//! coordinate converter (include/katana/cad/survey_tools.hpp,
//! src/katana_cad/survey_tools.cpp).
//!
//! NOTHING HERE IS NEW SURVEYING. Every number comes from katana_survey
//! (cogo, traverse, leveling, network_adjustment, angles) or katana_geodesy
//! (the PROJ transformer and grid factors); this module only moves drawing
//! entities in and out of those libraries and puts the answers into words. A
//! second inverse or a second compass rule written here would be a second
//! answer to the same question, and the two would drift.
//!
//! Each tool is a structured result plus ONE formatter. The Survey menu's
//! dialogs and the command line (INVERSE, FORWARD, AREA) both print what the
//! formatter returns, so a dialog and a typed command cannot describe one
//! computation two ways. The window gathers text and calls these; it computes
//! nothing survey-specific itself.
//!
//! Conventions, all inherited rather than invented here:
//!
//! * Drawing coordinates are x = easting, y = northing (as everywhere in the
//!   drawing and on the command line: "E,N"). katana_survey's Coordinate2 is
//!   (northing, easting); the conversion happens in this module only.
//!   Transposing the two gives a drawing that looks right and is wrong, so
//!   the tests use coordinates whose magnitudes differ.
//! * Angles are radians in every struct. Azimuths are clockwise from grid
//!   north in [0, 2*pi), as katana_survey's angles define them.
//! * Heights are Option. ABSENT IS NOT ZERO: an inverse between two points of
//!   which one has no height reports no height difference, never one measured
//!   from a datum nobody observed.
//! * Lengths are in drawing units, which the project metadata names
//!   (Document::metadata().linear_unit, "metre" unless a project says
//!   otherwise). Hectares are reported only when that unit is the metre.
//! * No scale factor, convergence or curvature is applied: these are plane
//!   (grid) computations, as katana_survey's cogo states. The coordinate
//!   converter REPORTS the grid scale factor and convergence; it does not
//!   apply them.
//!
//! Angle text (survey_angle_formats() is the one sentence every caller shows):
//! what katana_survey's parse_dms and parse_bearing accept. D M S with a mark
//! after each field (36°52'11.63", 36d52m11.63s, 36:52:11.63, 36-52-11.63, or
//! blank separated 36 52 11.63) or decimal degrees 36.8699; only the last
//! field may carry a fraction. The blank separated form is for a box that
//! holds one angle: in a line of several fields (a traverse leg, a converter
//! point) blanks separate the fields, so an angle there has none inside it. A
//! direction may also be a quadrant bearing, N 36d52m11.63s E. There is no
//! DDD.MMSS "calculator" notation, because 36.5211 would then mean two
//! different angles depending on who typed it.
//!
//! The report tables pad their columns by characters, where the C++'s
//! std::format pads by display width: a station id holding wide or combining
//! characters lines up otherwise (docs/rust.md, "Where the Rust necessarily
//! differs").

use std::collections::BTreeSet;
use std::sync::OnceLock;

use katana_commands::{CommandPtr, Transaction, create_entities};
use katana_core::error::make_error;
use katana_core::text::{
    format_exact_real, format_fixed, format_general, is_ascii_space, parse_finite_double,
    parse_integer, split_lines,
};
use katana_core::{Error, ErrorCode, Result, Status};
use katana_entity::entity::{heights_of, property_to_string, set_heights, to_string};
use katana_entity::{Entity, EntityId, EntityType, Geometry, PointGeometry, PropertyValue};
use katana_geodesy::coordinate_reference_system::to_string as kind_name;
use katana_geodesy::{
    Coordinate, CoordinateReferenceSystem, CoordinateTransformer, GridFactorCalculator,
    GridFactors, ProjectedCoordinate, TransformerOptions,
};
use katana_geometry::{Point2, Polyline2};
use katana_math::numerics::PI;
use katana_survey::angles::{
    arc_seconds_to_radians, degrees_to_radians, format_bearing, format_dms, gon_to_radians,
    normalize_azimuth, parse_bearing, parse_dms, radians_to_arc_seconds, radians_to_degrees,
    radians_to_gon,
};
use katana_survey::cogo::{forward, inverse, polygon_area, polygon_perimeter};
use katana_survey::leveling::{allowable_level_misclosure, compute_level_run};
use katana_survey::network_adjustment::adjust_traverse;
use katana_survey::traverse::compute_traverse;
use katana_survey::{
    AdjustmentOptions, Coordinate2, HorizontalAdjustmentResult, LevelAdjustment, LevelRun,
    LevelSetup, Traverse, TraverseAdjustment, TraverseClosingAngle, TraverseKind, TraverseOptions,
    TraverseResult, TraverseSetup, TraverseStochasticModel,
};

use crate::document::Document;
use crate::survey_import::SurveyImportOptions;

/// Seconds shown in every DMS angle and bearing these tools print: 0.01" is
/// 0.05 mm at 1 km, finer than any distance they print (1 mm), so rounding
/// the angle never hides a difference the distance shows.
pub const SURVEY_SECONDS_DECIMALS: i32 = 2;

// ---- the one crossing between the drawing and the survey library -------------------
//
// The drawing is (x, y) = (easting, northing); katana_survey's Coordinate2 is
// (northing, easting). Every conversion goes through these two, so there is
// exactly one place where the order could be wrong - and the tests use
// coordinates whose eastings and northings cannot be confused.

fn to_survey(point: Point2) -> Coordinate2 {
    Coordinate2::new(point.y, point.x)
}

fn from_survey(coordinate: Coordinate2) -> Point2 {
    Point2::new(coordinate.easting, coordinate.northing)
}

/// The property a point's name or number is read from and written to: the
/// one the survey import writes, taken from its options rather than spelt
/// again, so that a point this module creates and one a field file created
/// are named alike.
fn point_name_property() -> &'static str {
    static NAME: OnceLock<String> = OnceLock::new();
    NAME.get_or_init(|| SurveyImportOptions::default().point_number_property)
}

/// The C library's and std::format's fixed text, `%.Nf` and `{:.Nf}`:
/// katana_core's format_fixed with the sign of a negative value that rounds
/// to zero kept, as they keep it ("-0.000"). The vertex table shares it.
pub(crate) fn c_fixed(value: f64, decimals: i32) -> String {
    let text = format_fixed(value, decimals);
    if value.is_sign_negative() && !text.starts_with('-') {
        return format!("-{text}");
    }
    text
}

/// A value that rounds to zero is printed without a sign. PROJ and the
/// adjustments return -1e-10 for an exact zero often enough that "-0.000"
/// would appear in reports, where it reads as a small negative misclosure.
fn unsigned_zero(mut text: String) -> String {
    let rounds_to_zero = !text.bytes().any(|byte| (b'1'..=b'9').contains(&byte));
    if rounds_to_zero && (text.starts_with('-') || text.starts_with('+')) {
        text.remove(0);
    }
    text
}

fn fixed(value: f64, decimals: i32) -> String {
    unsigned_zero(c_fixed(value, decimals))
}

/// std::format's `{:+.Nf}`, then unsigned_zero.
fn signed_fixed(value: f64, decimals: i32) -> String {
    let text = c_fixed(value, decimals);
    if text.starts_with('-') {
        return unsigned_zero(text);
    }
    unsigned_zero(format!("+{text}"))
}

/// DMS and bearing text. The survey formatters fail only for a non-finite
/// angle, which nothing here produces; if one ever did, the report says so in
/// place rather than printing a plausible angle.
fn dms(radians: f64) -> String {
    match format_dms(radians, SURVEY_SECONDS_DECIMALS) {
        Ok(text) => text,
        Err(error) => format!("({})", error.message),
    }
}

fn bearing(azimuth: f64) -> String {
    match format_bearing(azimuth, SURVEY_SECONDS_DECIMALS) {
        Ok(text) => text,
        Err(error) => format!("({})", error.message),
    }
}

fn coordinate_text(point: Point2) -> String {
    format!("E {} N {}", fixed(point.x, 3), fixed(point.y, 3))
}

fn is_space(character: char) -> bool {
    u8::try_from(character).is_ok_and(is_ascii_space)
}

/// core's trimmed over text: it drops ASCII bytes only.
fn trimmed(text: &str) -> &str {
    text.trim_matches(is_space)
}

/// core's split_lines over text: its lines end at ASCII bytes, so each is
/// UTF-8 too.
fn lines_of(text: &str) -> Vec<&str> {
    split_lines(text.as_bytes())
        .into_iter()
        .map(|line| std::str::from_utf8(line).unwrap_or_default())
        .collect()
}

/// Blank- or comma-separated fields of one line of a field book, with a '#'
/// comment cut off. core's is_ascii_space rather than a locale's idea of a
/// blank: the locale must not decide what separates two readings.
fn fields(line: &str) -> Vec<&str> {
    let line = match line.find('#') {
        Some(hash) => &line[..hash],
        None => line,
    };
    let bytes = line.as_bytes();
    let separator = |byte: u8| is_ascii_space(byte) || byte == b',';
    let mut out = Vec::new();
    let mut start = 0;
    while start < bytes.len() {
        while start < bytes.len() && separator(bytes[start]) {
            start += 1;
        }
        let mut end = start;
        while end < bytes.len() && !separator(bytes[end]) {
            end += 1;
        }
        if end > start {
            out.push(&line[start..end]);
        }
        start = end;
    }
    out
}

fn line_context(line_number: usize, line: &str) -> String {
    format!("line {line_number}: {}", trimmed(line))
}

fn find_entity(document: &Document, id: EntityId) -> Option<&Entity> {
    document.model().entities.find(id)
}

/// An entity's type as words in a sentence: "a line", "an arc", "a text".
fn with_article(entity_type: EntityType) -> String {
    let name = to_string(entity_type).to_ascii_lowercase();
    let vowel = name.starts_with(['a', 'e', 'i', 'o', 'u']);
    format!("{} {name}", if vowel { "an" } else { "a" })
}

fn name_of(entity: &Entity) -> String {
    entity
        .properties
        .get(point_name_property())
        .map_or_else(String::new, property_to_string)
}

/// A direction given as a mark or an azimuth; see TraverseOrientation.
fn orientation_azimuth(
    station: Point2,
    orientation: &TraverseOrientation,
    what: &str,
) -> Result<f64> {
    let Some(mark) = orientation.reference_mark else {
        if !orientation.azimuth.is_finite() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                format!("{what} azimuth is not finite"),
            ));
        }
        return Ok(normalize_azimuth(orientation.azimuth));
    };
    match inverse(to_survey(station), to_survey(mark)) {
        Ok(found) => Ok(found.azimuth),
        Err(error) => Err(make_error(
            ErrorCode::InvalidArgument,
            format!(
                "{what} reference mark gives no direction: {}",
                error.message
            ),
            coordinate_text(mark),
        )),
    }
}

// ---- text in ---------------------------------------------------------------------------

/// The accepted angle formats, in one sentence, for error messages and
/// dialogs.
pub fn survey_angle_formats() -> &'static str {
    "an angle is degrees, minutes and seconds with a mark after each - 36\u{B0}52'11.63\", \
     36d52m11.63s, 36:52:11.63 or 36-52-11.63 - or decimal degrees (36.8699); a direction \
     may also be a quadrant bearing, N 36d52m11.63s E"
}

/// An angle (katana_survey's parse_dms): radians, sign kept. ParseFailure
/// naming the accepted formats otherwise.
pub fn parse_survey_angle(text: &str) -> Result<f64> {
    parse_dms(text).map_err(|error| {
        make_error(
            ErrorCode::ParseFailure,
            format!("{}; {}", error.message, survey_angle_formats()),
            text,
        )
    })
}

/// A direction: a quadrant bearing (N ... E, katana_survey's parse_bearing)
/// or an azimuth in any parse_survey_angle format. An azimuth in [0, 2*pi).
pub fn parse_survey_direction(text: &str) -> Result<f64> {
    let body = trimmed(text);
    // A quadrant bearing starts with its meridian: nothing else an angle can
    // start with is a letter, so the first character decides.
    if matches!(body.as_bytes().first(), Some(b'N' | b'n' | b'S' | b's')) {
        return parse_bearing(body).map_err(|error| {
            make_error(
                ErrorCode::ParseFailure,
                format!("{}; {}", error.message, survey_angle_formats()),
                text,
            )
        });
    }
    Ok(normalize_azimuth(parse_survey_angle(body)?))
}

/// A finite number, locale-independently (core's parse_finite_double): "1.5"
/// is never "1,5". ParseFailure naming `what` otherwise.
pub fn parse_survey_number(text: &str, what: &str) -> Result<f64> {
    parse_finite_double(trimmed(text)).ok_or_else(|| {
        make_error(
            ErrorCode::ParseFailure,
            format!("{what} must be a number ('.' is the decimal point)"),
            text,
        )
    })
}

// ---- positions -------------------------------------------------------------------------

/// A place a survey tool works from: typed coordinates, a point entity, or
/// one end of a line.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SurveyPosition {
    pub point: Point2,          // x easting, y northing, drawing units
    pub elevation: Option<f64>, // absent is not zero
    /// The entity it was read from; None for typed coordinates.
    pub entity: Option<EntityId>,
    /// What it is on that entity: "point 7", "line 5 start". Empty when typed.
    pub source: String,
    /// The point's name or number where the entity carries one (the property
    /// the survey import writes, SurveyImportOptions::point_number_property).
    pub name: String,
}

impl SurveyPosition {
    /// How a report names it: "point 7 (CP-01)", "point 7", "E 3.000 N 4.000".
    pub fn label(&self) -> String {
        let Some(entity) = self.entity else {
            let mut text = coordinate_text(self.point);
            if let Some(elevation) = self.elevation {
                text.push_str(&format!(" Z {}", fixed(elevation, 3)));
            }
            return text;
        };
        let mut text = if self.source.is_empty() {
            format!("entity {entity}")
        } else {
            self.source.clone()
        };
        if !self.name.is_empty() {
            text.push_str(&format!(" ({})", self.name));
        }
        text
    }
}

/// A point entity. NotFound for an unknown id; InvalidArgument for an entity
/// that is not a point (the message says what it is instead).
pub fn position_of_point(document: &Document, id: EntityId) -> Result<SurveyPosition> {
    let Some(entity) = find_entity(document, id) else {
        return Err(make_error(
            ErrorCode::NotFound,
            "no entity with that id",
            format!("{id}"),
        ));
    };
    let Geometry::Point(point) = &entity.geometry else {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!(
                "a survey position must be a point entity; this is {}",
                with_article(entity.r#type())
            ),
            format!("{id}"),
        ));
    };
    Ok(SurveyPosition {
        point: point.position,
        elevation: heights_of(&entity.properties, 1)[0],
        entity: Some(id),
        source: format!("point {id}"),
        name: name_of(entity),
    })
}

/// The two ends of a line, or the first and last vertex of an open polyline,
/// with the heights the entity carries for them (entity's heights_of).
/// InvalidArgument for anything else, including a closed polyline, whose ends
/// are the same place.
pub fn ends_of_line(document: &Document, id: EntityId) -> Result<(SurveyPosition, SurveyPosition)> {
    let Some(entity) = find_entity(document, id) else {
        return Err(make_error(
            ErrorCode::NotFound,
            "no entity with that id",
            format!("{id}"),
        ));
    };
    let (vertices, kind): (Vec<Point2>, &str) = match &entity.geometry {
        Geometry::Line(segment) => (vec![segment.start, segment.end], "line "),
        Geometry::Polyline(polyline) if !polyline.closed && polyline.vertices.len() >= 2 => {
            (polyline.vertices.clone(), "polyline ")
        }
        _ => {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                format!(
                    "the two ends of a line or of an open polyline are needed; this is {}",
                    with_article(entity.r#type())
                ),
                format!("{id}"),
            ));
        }
    };
    let heights = heights_of(&entity.properties, vertices.len());
    let start = SurveyPosition {
        point: vertices[0],
        elevation: heights[0],
        entity: Some(id),
        source: format!("{kind}{id} start"),
        name: String::new(),
    };
    let end = SurveyPosition {
        point: vertices[vertices.len() - 1],
        elevation: heights[heights.len() - 1],
        entity: Some(id),
        source: format!("{kind}{id} end"),
        name: String::new(),
    };
    Ok((start, end))
}

/// "E,N", "E,N,Z" (easting first, as every point typed into this program
/// is) or the id of a point entity. ParseFailure / NotFound /
/// InvalidArgument.
pub fn parse_survey_position(document: &Document, text: &str) -> Result<SurveyPosition> {
    let body = trimmed(text);
    if !body.contains(',') {
        // No comma: an entity id, as LIST shows them.
        return match parse_integer(body) {
            Some(id) if id > 0 => position_of_point(document, id as EntityId),
            _ => Err(make_error(
                ErrorCode::ParseFailure,
                "a position is E,N or E,N,Z (easting first) or a point's entity id",
                text,
            )),
        };
    }
    let mut numbers = Vec::new();
    for part in body.split(',') {
        let Some(value) = parse_finite_double(trimmed(part)) else {
            return Err(make_error(
                ErrorCode::ParseFailure,
                "a position is E,N or E,N,Z with numbers ('.' is the decimal point)",
                text,
            ));
        };
        numbers.push(value);
    }
    if !(2..=3).contains(&numbers.len()) {
        return Err(make_error(
            ErrorCode::ParseFailure,
            "a position is E,N or E,N,Z (easting first)",
            text,
        ));
    }
    Ok(SurveyPosition {
        point: Point2::new(numbers[0], numbers[1]),
        elevation: numbers.get(2).copied(),
        ..SurveyPosition::default()
    })
}

/// The selected point entities, in id order (the order SelectionSet keeps).
/// Everything selected that is not a point is left out; the dialogs say how
/// many were.
pub fn selected_point_positions(document: &Document) -> Vec<SurveyPosition> {
    document
        .selection()
        .ids()
        .into_iter()
        .filter_map(|id| position_of_point(document, id).ok())
        .collect()
}

// ---- inverse ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq)]
pub struct InverseResult {
    pub from: SurveyPosition,
    pub to: SurveyPosition,
    pub delta_easting: f64,  // to - from
    pub delta_northing: f64, // to - from
    pub horizontal_distance: f64,
    /// Radians, [0, 2*pi), from katana_survey's inverse.
    pub azimuth: f64,
    /// All three need a height at BOTH ends; None otherwise.
    pub height_difference: Option<f64>, // to - from
    pub slope_distance: Option<f64>,
    pub grade: Option<f64>, // height_difference / horizontal_distance
}

/// katana_survey's inverse between two positions. InvalidArgument when they
/// are the same ground mark (closer than 0.1 mm): the azimuth between them is
/// undefined and a report of "0.000 at 0°" would be a made-up answer.
pub fn compute_inverse(from: &SurveyPosition, to: &SurveyPosition) -> Result<InverseResult> {
    let found = inverse(to_survey(from.point), to_survey(to.point)).map_err(|error| {
        make_error(
            error.code,
            error.message,
            format!("{} to {}", from.label(), to.label()),
        )
    })?;
    let mut result = InverseResult {
        from: from.clone(),
        to: to.clone(),
        delta_easting: to.point.x - from.point.x,
        delta_northing: to.point.y - from.point.y,
        horizontal_distance: found.distance,
        azimuth: found.azimuth,
        ..InverseResult::default()
    };
    if let (Some(start), Some(end)) = (from.elevation, to.elevation) {
        let dz = end - start;
        result.height_difference = Some(dz);
        result.slope_distance = Some(result.horizontal_distance.hypot(dz));
        // katana_survey's inverse refuses coincident marks, so the distance is
        // at least the 0.1 mm coordinate tolerance and the ratio is finite.
        result.grade = Some(dz / result.horizontal_distance);
    }
    Ok(result)
}

pub fn format_inverse_report(result: &InverseResult) -> String {
    let mut text = format!(
        "Inverse  {}  ->  {}\n",
        result.from.label(),
        result.to.label()
    );
    text.push_str(&format!(
        "  dE {}   dN {}\n",
        signed_fixed(result.delta_easting, 3),
        signed_fixed(result.delta_northing, 3)
    ));
    text.push_str(&format!(
        "  Horizontal distance {}\n",
        fixed(result.horizontal_distance, 3)
    ));
    text.push_str(&format!(
        "  Azimuth {}   back azimuth {}\n",
        dms(result.azimuth),
        dms(normalize_azimuth(result.azimuth + PI))
    ));
    text.push_str(&format!("  Bearing {}", bearing(result.azimuth)));
    match (
        result.height_difference,
        result.slope_distance,
        result.grade,
    ) {
        (Some(height_difference), Some(slope_distance), Some(grade)) => {
            text.push_str(&format!(
                "\n  Height difference {}   slope distance {}   grade {} %",
                signed_fixed(height_difference, 3),
                fixed(slope_distance, 3),
                signed_fixed(grade * 100.0, 3)
            ));
        }
        _ => {
            // Said, not left out: a missing line reads as "flat". Naming one
            // end when neither has a height would suggest the other has one.
            if result.from.elevation.is_none() && result.to.elevation.is_none() {
                text.push_str("\n  No height difference: neither end has an elevation");
            } else {
                let missing = if result.from.elevation.is_some() {
                    &result.to
                } else {
                    &result.from
                };
                text.push_str(&format!(
                    "\n  No height difference: {} has no elevation",
                    missing.label()
                ));
            }
        }
    }
    text
}

// ---- forward (radiate) -----------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ForwardInput {
    pub from: SurveyPosition,
    pub azimuth: f64,  // radians, any finite value
    pub distance: f64, // horizontal, drawing units, > 0
    /// The new point's height is from.elevation + this. Given with a start
    /// that has no height, it is REFUSED rather than taken from zero.
    pub height_difference: Option<f64>,
    pub name: String, // written as the new point's name; may be empty
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ForwardResult {
    pub input: ForwardInput,
    pub point: Point2, // x easting, y northing
    pub elevation: Option<f64>,
}

/// katana_survey's forward. InvalidArgument for a non-positive or non-finite
/// distance, a non-finite azimuth, or a height difference from a start with
/// no height.
pub fn compute_forward(input: &ForwardInput) -> Result<ForwardResult> {
    if !input.distance.is_finite() || !(input.distance > 0.0) {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "the horizontal distance must be a positive number",
            format_exact_real(input.distance),
        ));
    }
    if let Some(height_difference) = input.height_difference {
        if !height_difference.is_finite() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "the height difference is not finite",
            ));
        }
        if input.from.elevation.is_none() {
            // Absent is not zero: a height difference from a mark with no
            // height would put the new point at a height nobody measured.
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "a height difference was given but the start has no elevation",
                input.from.label(),
            ));
        }
    }
    let reached = forward(to_survey(input.from.point), input.azimuth, input.distance)?;
    let mut result = ForwardResult {
        input: input.clone(),
        point: from_survey(reached),
        elevation: None,
    };
    result.input.azimuth = normalize_azimuth(input.azimuth);
    if let (Some(height_difference), Some(start)) = (input.height_difference, input.from.elevation)
    {
        result.elevation = Some(start + height_difference);
    }
    Ok(result)
}

pub fn format_forward_report(result: &ForwardResult) -> String {
    let input = &result.input;
    let mut text = format!("Forward from {}\n", input.from.label());
    text.push_str(&format!(
        "  Azimuth {}   bearing {}\n",
        dms(input.azimuth),
        bearing(input.azimuth)
    ));
    text.push_str(&format!(
        "  Horizontal distance {}",
        fixed(input.distance, 3)
    ));
    if let Some(height_difference) = input.height_difference {
        text.push_str(&format!(
            "   height difference {}",
            signed_fixed(height_difference, 3)
        ));
    }
    let name = if input.name.is_empty() {
        String::new()
    } else {
        format!(" {}", input.name)
    };
    text.push_str(&format!(
        "\n  New point{name}  {}",
        coordinate_text(result.point)
    ));
    match result.elevation {
        Some(elevation) => text.push_str(&format!(" Z {}", fixed(elevation, 3))),
        None => text.push_str("  (no elevation)"),
    }
    text
}

/// The new point as ONE undoable command on the document's current layer,
/// with its name and height as the survey import writes them.
pub fn forward_point_command(document: &Document, result: &ForwardResult) -> Result<CommandPtr> {
    let attributes = document.current_attributes();
    let mut entity = Entity {
        geometry: Geometry::Point(PointGeometry {
            position: result.point,
        }),
        layer: attributes.layer,
        style: attributes.style,
        color: attributes.color,
        ..Entity::default()
    };
    if !result.input.name.is_empty() {
        entity.properties.insert(
            String::from(point_name_property()),
            PropertyValue::Text(result.input.name.clone()),
        );
    }
    // The one writer of heights (entity.rs): no property at all for a point
    // without one, rather than 0.0.
    set_heights(&mut entity.properties, &[result.elevation]);
    let mut transaction = Transaction::new("SURVEY_FORWARD_POINT");
    transaction.add(create_entities(vec![entity]));
    Ok(Box::new(transaction))
}

// ---- area ------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct AreaItem {
    pub id: EntityId,
    pub r#type: EntityType,
    pub area: f64,      // square drawing units, >= 0
    pub perimeter: f64, // drawing units
}

impl Default for AreaItem {
    fn default() -> Self {
        Self {
            id: katana_entity::entity::INVALID_ENTITY_ID,
            r#type: EntityType::Polyline,
            area: 0.0,
            perimeter: 0.0,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AreaSkip {
    pub id: EntityId,
    pub reason: String, // "an open polyline has no area", ...
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AreaResult {
    pub items: Vec<AreaItem>, // in the order asked
    pub skipped: Vec<AreaSkip>,
    pub total_area: f64,
    pub total_perimeter: f64,
    /// The project's linear unit, as its metadata names it, and whether that
    /// is the metre - which is the only case in which hectares mean anything.
    pub linear_unit: String,
    pub metres: bool,
}

/// True for "metre", "meter", "metres", "meters" and "m", in any case: the
/// spellings a project's linear_unit is written with.
pub fn is_metre_unit(linear_unit: &str) -> bool {
    let unit = trimmed(linear_unit).to_ascii_lowercase();
    matches!(unit.as_str(), "metre" | "meter" | "metres" | "meters" | "m")
}

/// The skipped entities grouped by reason, in the order the reasons are met,
/// each as "N: reason - id, id, ..." with at most a dozen ids and "and M
/// more" for the rest. The report and the nothing-has-an-area error both
/// print these: Area of Selection over a whole drawing of points would
/// otherwise put a line, or one error thousands of entries long, in the log
/// for every point in it. Every skipped entity is still counted.
fn skipped_by_reason(skipped: &[AreaSkip]) -> Vec<String> {
    let mut groups: Vec<(&str, Vec<EntityId>)> = Vec::new();
    for skip in skipped {
        match groups.iter_mut().find(|(reason, _)| *reason == skip.reason) {
            Some((_, ids)) => ids.push(skip.id),
            None => groups.push((skip.reason.as_str(), vec![skip.id])),
        }
    }
    const IDS_SHOWN: usize = 12;
    let mut out = Vec::with_capacity(groups.len());
    for (reason, ids) in groups {
        let mut text = format!("{}: {reason} - ", ids.len());
        for (i, id) in ids.iter().take(IDS_SHOWN).enumerate() {
            if i > 0 {
                text.push_str(", ");
            }
            text.push_str(&format!("{id}"));
        }
        if ids.len() > IDS_SHOWN {
            text.push_str(&format!(" and {} more", ids.len() - IDS_SHOWN));
        }
        out.push(text);
    }
    out
}

/// Closed polylines (katana_survey's polygon_area and polygon_perimeter,
/// which work relative to the first vertex so UTM-sized coordinates keep
/// their precision) and circles. Everything else, and a degenerate polygon,
/// is skipped with the reason; an id listed more than once is measured once
/// and each repeat is skipped as "listed more than once", so no outline is
/// added to the total twice. InvalidArgument for an empty list; NotFound for
/// an unknown id; InvalidGeometry when nothing in the list has an area - with
/// the reasons, grouped and shortened as format_area_report lists them (a
/// count per reason and at most a dozen ids), so a selection of thousands of
/// points is one short line. Self-intersecting outlines are not detected: the
/// signed areas of their lobes cancel (katana_survey's cogo), which the report
/// states.
pub fn compute_area(document: &Document, ids: &[EntityId]) -> Result<AreaResult> {
    if ids.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "nothing to measure; select closed polylines or circles",
        ));
    }
    let linear_unit = document.metadata().linear_unit.clone();
    let mut result = AreaResult {
        metres: is_metre_unit(&linear_unit),
        linear_unit,
        ..AreaResult::default()
    };
    // AREA passes typed ids straight through, and an id typed twice would
    // add its outline to the total twice. Each entity is measured once; the
    // repeat is reported as skipped rather than dropped without a word.
    let mut seen: BTreeSet<EntityId> = BTreeSet::new();
    for &id in ids {
        if !seen.insert(id) {
            result.skipped.push(AreaSkip {
                id,
                reason: String::from("listed more than once"),
            });
            continue;
        }
        let Some(entity) = find_entity(document, id) else {
            return Err(make_error(
                ErrorCode::NotFound,
                "no entity with that id",
                format!("{id}"),
            ));
        };
        let item = |r#type: EntityType, area: f64, perimeter: f64| AreaItem {
            id,
            r#type,
            area,
            perimeter,
        };
        let skip = |reason: String| AreaSkip { id, reason };
        match &entity.geometry {
            Geometry::Circle(circle) => {
                result
                    .items
                    .push(item(EntityType::Circle, circle.area(), circle.perimeter()));
            }
            // The drawing system's closed figures (docs/drawing.md): exact,
            // the arcs' segments of area included, not their chords.
            Geometry::CurvePolyline(curve) => {
                if !curve.closed || curve.vertices.len() < 3 {
                    result
                        .skipped
                        .push(skip(String::from("an open polyline has no area")));
                } else {
                    result.items.push(item(
                        EntityType::CurvePolyline,
                        curve.area(),
                        curve.length(),
                    ));
                }
            }
            Geometry::Ellipse(ellipse) => {
                if !ellipse.is_full() {
                    result
                        .skipped
                        .push(skip(String::from("an elliptical arc has no area")));
                } else {
                    result.items.push(item(
                        EntityType::Ellipse,
                        PI * ellipse.major_radius() * ellipse.minor_radius(),
                        ellipse.length(),
                    ));
                }
            }
            Geometry::Polyline(polyline) => {
                if !polyline.closed {
                    result
                        .skipped
                        .push(skip(String::from("an open polyline has no area")));
                    continue;
                }
                let vertices: Vec<Coordinate2> = polyline
                    .vertices
                    .iter()
                    .map(|&vertex| to_survey(vertex))
                    .collect();
                let area = polygon_area(&vertices);
                let perimeter = polygon_perimeter(&vertices);
                match (area, perimeter) {
                    (Ok(area), Ok(perimeter)) => {
                        result
                            .items
                            .push(item(EntityType::Polyline, area, perimeter));
                    }
                    (area, perimeter) => {
                        let error = match area {
                            Err(error) => error,
                            Ok(_) => perimeter.err().unwrap_or_default(),
                        };
                        result
                            .skipped
                            .push(skip(format!("not a polygon: {}", error.message)));
                    }
                }
            }
            _ => {
                result.skipped.push(skip(format!(
                    "{} has no area",
                    with_article(entity.r#type())
                )));
            }
        }
    }
    if result.items.is_empty() {
        let mut reasons = String::new();
        for group in skipped_by_reason(&result.skipped) {
            reasons.push_str(if reasons.is_empty() {
                "skipped "
            } else {
                "; skipped "
            });
            reasons.push_str(&group);
        }
        return Err(make_error(
            ErrorCode::InvalidGeometry,
            "none of these is a closed polyline or a circle",
            reasons,
        ));
    }
    for item in &result.items {
        result.total_area += item.area;
        result.total_perimeter += item.perimeter;
    }
    Ok(result)
}

pub fn format_area_report(result: &AreaResult) -> String {
    // One hectare is 10 000 m^2 by definition (SI Brochure, 9th edition,
    // table 8: the hectare, accepted for use with the SI).
    const SQUARE_METRES_PER_HECTARE: f64 = 10_000.0;
    let square_unit = if result.metres {
        String::from("m\u{B2}")
    } else {
        format!("square {}", result.linear_unit)
    };
    let length_unit = if result.metres {
        String::from("m")
    } else {
        result.linear_unit.clone()
    };
    let area = |value: f64| {
        let mut text = format!("{} {square_unit}", fixed(value, 3));
        if result.metres {
            text.push_str(&format!(
                " ({} ha)",
                fixed(value / SQUARE_METRES_PER_HECTARE, 4)
            ));
        }
        text
    };

    let mut text = format!(
        "Area of {}{} - drawing unit '{}' (project settings)\n",
        result.items.len(),
        if result.items.len() == 1 {
            " outline"
        } else {
            " outlines"
        },
        result.linear_unit
    );
    for item in &result.items {
        let kind = match item.r#type {
            EntityType::Circle => "  circle    ",
            EntityType::Ellipse => "  ellipse   ",
            _ => "  polyline  ",
        };
        text.push_str(&format!(
            "  {}{kind}area {}   perimeter {} {length_unit}\n",
            item.id,
            area(item.area),
            fixed(item.perimeter, 3)
        ));
    }
    text.push_str(&format!(
        "  Total  area {}   perimeter {} {length_unit}",
        area(result.total_area),
        fixed(result.total_perimeter, 3)
    ));
    for group in skipped_by_reason(&result.skipped) {
        text.push_str(&format!("\n  Skipped {group}"));
    }
    if !result.metres {
        text.push_str("\n  No hectares: they are defined only for a drawing in metres");
    }
    text.push_str("\n  A self-intersecting outline is not detected; the areas of its lobes cancel");
    text
}

// ---- traverse --------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TraverseMethod {
    /// Misclosures only.
    #[default]
    None,
    /// Bowditch: corrections proportional to leg length.
    Compass,
    /// Corrections proportional to |latitude| and |departure|.
    Transit,
    /// katana_survey's adjust_traverse, weighted by `precision`.
    LeastSquares,
}

impl katana_entity::EnumName for TraverseMethod {
    fn name(self) -> &'static str {
        match self {
            TraverseMethod::None => "no adjustment",
            TraverseMethod::Compass => "Bowditch (compass) rule",
            TraverseMethod::Transit => "transit rule",
            TraverseMethod::LeastSquares => "least squares",
        }
    }
}

/// A direction a traverse is hung on: a known azimuth, or a reference mark
/// whose azimuth from the station is computed (katana_survey's inverse). The
/// mark wins when both are given, because a mark is what a surveyor actually
/// sights.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TraverseOrientation {
    pub reference_mark: Option<Point2>, // x easting, y northing
    pub azimuth: f64,                   // radians
}

/// The a-priori precision the least-squares method weights with. The
/// defaults are a common construction total-station specification - 5"
/// angles and 2 mm + 2 ppm distances (the class manufacturers sell as "5
/// second") - and are there to be REPLACED with the instrument's own; the
/// report prints them so a result is never read without the weights it came
/// from. The azimuth sigma of 0.01" holds a loop's first-leg azimuth
/// practically fixed, the value katana_survey's network_adjustment suggests
/// for it.
pub fn default_traverse_precision() -> TraverseStochasticModel {
    TraverseStochasticModel {
        distance_sigma_constant: 0.002, // 2 mm
        distance_sigma_ppm: 2.0,        // + 2 ppm
        angle_sigma: arc_seconds_to_radians(5.0),
        azimuth_sigma: arc_seconds_to_radians(0.01),
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TraverseSpec {
    pub name: String,
    pub kind: TraverseKind,
    /// Known coordinates of legs[0].station_id.
    pub start: Point2,
    /// Open and Link: the direction from the start station to its backsight;
    /// legs[0].angle is turned from it. ClosedLoop: the azimuth of the first
    /// leg (known or assumed), as katana_survey's Traverse defines it.
    pub start_orientation: TraverseOrientation,
    /// One per occupied station, in order: its id, the angle turned clockwise
    /// from the back station to the forward one, and the horizontal distance
    /// to the forward station. A loop's last leg returns to its first
    /// station.
    pub legs: Vec<TraverseSetup>,
    /// Open and Link: the station the last leg ends on. Link: its known
    /// coordinates, and optionally the angle turned there from the last
    /// traverse station to a reference mark, with that mark's direction - the
    /// angular check.
    pub end_station: String,
    pub end: Point2,
    pub closing_angle: Option<f64>,
    pub closing_orientation: TraverseOrientation,
    pub method: TraverseMethod,
    /// Distribute the angular misclosure equally over the angles first (the
    /// classical methods; least squares weighs the angles itself).
    pub balance_angles: bool,
    pub precision: TraverseStochasticModel,
}

impl Default for TraverseSpec {
    fn default() -> Self {
        Self {
            name: String::from("Traverse"),
            kind: TraverseKind::Open,
            start: Point2::default(),
            start_orientation: TraverseOrientation::default(),
            legs: Vec::new(),
            end_station: String::new(),
            end: Point2::default(),
            closing_angle: None,
            closing_orientation: TraverseOrientation::default(),
            method: TraverseMethod::Compass,
            balance_angles: true,
            precision: default_traverse_precision(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TraverseStationRow {
    pub id: String,
    pub unadjusted: Point2, // from the (balanced) angles and observed distances
    pub adjusted: Point2,   // after the chosen method
    /// Least squares only: the adjusted standard deviations (zero for
    /// control).
    pub sigma_easting: Option<f64>,
    pub sigma_northing: Option<f64>,
    /// Start and closing control: known, not computed. Not drawn as new
    /// points.
    pub control: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TraverseToolResult {
    pub spec: TraverseSpec,
    pub start_azimuth: f64, // the orientation actually used, radians
    pub closing_reference_azimuth: Option<f64>,
    /// katana_survey's compute_traverse with the classical method (None for
    /// least squares): the angular and linear misclosures and the precision
    /// ratio.
    pub classical: TraverseResult,
    pub least_squares: Option<HorizontalAdjustmentResult>,
    pub stations: Vec<TraverseStationRow>, // traverse order, the start first
}

/// Builds katana_survey's Traverse and runs the chosen method on it. The
/// errors are validate_traverse's (a repeated station, a missing end, ...)
/// and adjust_traverse's, unchanged, plus InvalidArgument for a reference
/// mark on top of its station (no direction).
pub fn compute_traverse_tool(spec: &TraverseSpec) -> Result<TraverseToolResult> {
    let Some(first_leg) = spec.legs.first() else {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "a traverse needs at least one leg",
            format!("traverse '{}'", spec.name),
        ));
    };
    let mut result = TraverseToolResult {
        spec: spec.clone(),
        ..TraverseToolResult::default()
    };

    let looped = spec.kind == TraverseKind::ClosedLoop;
    let start_azimuth = orientation_azimuth(
        spec.start,
        &spec.start_orientation,
        if looped { "first-leg" } else { "backsight" },
    )?;
    result.start_azimuth = start_azimuth;

    let mut traverse = Traverse {
        name: spec.name.clone(),
        kind: spec.kind,
        start: to_survey(spec.start),
        start_azimuth,
        setups: spec.legs.clone(),
        ..Traverse::default()
    };
    if !looped {
        traverse.end_station_id = spec.end_station.clone();
    }
    if spec.kind == TraverseKind::Link {
        traverse.end = to_survey(spec.end);
        if let Some(closing_angle) = spec.closing_angle {
            let reference =
                orientation_azimuth(spec.end, &spec.closing_orientation, "closing reference")?;
            result.closing_reference_azimuth = Some(reference);
            traverse.closing_angle = Some(TraverseClosingAngle {
                angle: closing_angle,
                reference_azimuth: reference,
            });
        }
    }

    let options = TraverseOptions {
        balance_angles: spec.balance_angles,
        adjustment: match spec.method {
            TraverseMethod::Compass => TraverseAdjustment::Compass,
            TraverseMethod::Transit => TraverseAdjustment::Transit,
            // Least squares is reported beside the classical misclosures,
            // which are what a surveyor checks the field work against before
            // adjusting at all.
            TraverseMethod::None | TraverseMethod::LeastSquares => TraverseAdjustment::None,
        },
    };
    result.classical = compute_traverse(&traverse, &options)?;

    if spec.method == TraverseMethod::LeastSquares {
        result.least_squares = Some(adjust_traverse(
            &traverse,
            &spec.precision,
            &AdjustmentOptions::default(),
        )?);
    }

    let first = &first_leg.station_id;
    let mut stations = Vec::with_capacity(result.classical.stations.len());
    for station in &result.classical.stations {
        let mut row = TraverseStationRow {
            id: station.id.clone(),
            unadjusted: from_survey(station.unadjusted),
            adjusted: from_survey(station.adjusted),
            control: station.id == *first
                || (spec.kind == TraverseKind::Link && station.id == spec.end_station),
            ..TraverseStationRow::default()
        };
        if let Some(least_squares) = &result.least_squares {
            let Some(found) = least_squares
                .stations
                .iter()
                .find(|adjusted| adjusted.point_id == station.id)
            else {
                return Err(make_error(
                    ErrorCode::Internal,
                    "the least-squares adjustment has no result for a station",
                    station.id.as_str(),
                ));
            };
            row.adjusted = from_survey(found.position);
            row.sigma_easting = Some(found.sigma_easting);
            row.sigma_northing = Some(found.sigma_northing);
        }
        stations.push(row);
    }
    result.stations = stations;
    Ok(result)
}

pub fn format_traverse_report(result: &TraverseToolResult) -> String {
    let spec = &result.spec;
    let classical = &result.classical;
    let kind = match spec.kind {
        TraverseKind::ClosedLoop => "closed loop",
        TraverseKind::Link => "link traverse",
        TraverseKind::Open => "open traverse",
    };
    let first_station = spec.legs.first().map_or("", |leg| leg.station_id.as_str());
    let mut text = format!(
        "Traverse '{}' - {kind}, {}{}{}\n",
        spec.name,
        spec.legs.len(),
        if spec.legs.len() == 1 {
            " leg, "
        } else {
            " legs, "
        },
        to_string(spec.method)
    );
    text.push_str(&format!(
        "  Start {first_station}  {}, {}{}\n",
        coordinate_text(spec.start),
        if spec.kind == TraverseKind::ClosedLoop {
            "first-leg azimuth "
        } else {
            "backsight azimuth "
        },
        dms(result.start_azimuth)
    ));
    if spec.kind == TraverseKind::Link {
        text.push_str(&format!(
            "  Closing control {}  {}",
            spec.end_station,
            coordinate_text(spec.end)
        ));
        match result.closing_reference_azimuth {
            Some(reference) => {
                text.push_str(&format!(", reference azimuth {}\n", dms(reference)));
            }
            None => text.push_str(", no closing angle (no angular check)\n"),
        }
    }

    match classical.angular_misclosure {
        Some(misclosure) => {
            text.push_str(&format!(
                "  Angular misclosure {} over {} angles",
                dms(misclosure),
                classical.angle_count
            ));
            if spec.balance_angles {
                text.push_str(&format!(
                    "; {} applied to each\n",
                    dms(classical.angle_correction)
                ));
            } else {
                text.push_str("; not balanced\n");
            }
        }
        None => text.push_str("  No angular check\n"),
    }
    match &classical.linear_misclosure {
        Some(linear) => {
            text.push_str(&format!(
                "  Linear misclosure dN {}  dE {}  length {}  over {}",
                signed_fixed(linear.latitude, 3),
                signed_fixed(linear.departure, 3),
                fixed(linear.length, 3),
                fixed(classical.total_length, 3)
            ));
            match linear.precision_denominator {
                Some(denominator) => {
                    text.push_str(&format!("  precision 1 : {}\n", c_fixed(denominator, 0)));
                }
                None => text.push_str("  (exact closure)\n"),
            }
        }
        None => text.push_str("  Open traverse: no linear misclosure - nothing checks it\n"),
    }

    text.push_str(
        "  Legs\n    from        to          azimuth            distance    dN          dE",
    );
    let corrected =
        spec.method == TraverseMethod::Compass || spec.method == TraverseMethod::Transit;
    text.push_str(if corrected {
        "          corr N    corr E\n"
    } else {
        "\n"
    });
    for leg in &classical.legs {
        text.push_str(&format!(
            "    {:<11} {:<11} {:<18} {:>10}  {:>10}  {:>10}",
            leg.from_id,
            leg.to_id,
            dms(leg.azimuth),
            fixed(leg.distance, 3),
            signed_fixed(leg.latitude, 3),
            signed_fixed(leg.departure, 3)
        ));
        if corrected {
            text.push_str(&format!(
                "  {:>8}  {:>8}",
                signed_fixed(leg.latitude_correction, 4),
                signed_fixed(leg.departure_correction, 4)
            ));
        }
        text.push('\n');
    }

    text.push_str(
        "  Stations            unadjusted E   unadjusted N     adjusted E     adjusted N",
    );
    text.push_str(if result.least_squares.is_some() {
        "   sE (mm)   sN (mm)\n"
    } else {
        "\n"
    });
    for row in &result.stations {
        text.push_str(&format!(
            "    {:<16} {:>14} {:>14} {:>14} {:>14}",
            row.id,
            fixed(row.unadjusted.x, 3),
            fixed(row.unadjusted.y, 3),
            fixed(row.adjusted.x, 3),
            fixed(row.adjusted.y, 3)
        ));
        if let (Some(sigma_easting), Some(sigma_northing)) = (row.sigma_easting, row.sigma_northing)
        {
            text.push_str(&format!(
                "  {:>8}  {:>8}",
                fixed(sigma_easting * 1000.0, 1),
                fixed(sigma_northing * 1000.0, 1)
            ));
        }
        text.push_str(if row.control { "  control\n" } else { "\n" });
    }

    if let Some(least_squares) = &result.least_squares {
        let statistics = &least_squares.statistics;
        let precision = &spec.precision;
        text.push_str(&format!(
            "  Least squares weighted with angles {}\", distances {} mm + {} ppm; {} degrees of freedom, {} iterations",
            fixed(radians_to_arc_seconds(precision.angle_sigma), 1),
            fixed(precision.distance_sigma_constant * 1000.0, 1),
            fixed(precision.distance_sigma_ppm, 1),
            statistics.degrees_of_freedom,
            least_squares.iterations
        ));
        if let Some(variance_factor) = statistics.variance_factor {
            text.push_str(&format!("; variance factor {}", fixed(variance_factor, 3)));
        }
        if let Some(global_test) = &statistics.global_test {
            text.push_str(if global_test.passed {
                "; global test passed"
            } else {
                "; global test FAILED - check the weights and look for a gross error"
            });
        }
        text.push('\n');
    }
    if text.ends_with('\n') {
        text.pop();
    }
    text
}

/// ONE undoable command: a point entity for every computed (non-control)
/// station, named by its id, and the traverse as one polyline through every
/// station in order - closed for a loop. On the document's current layer.
pub fn traverse_command(document: &Document, result: &TraverseToolResult) -> Result<CommandPtr> {
    if result.stations.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the traverse has no stations",
        ));
    }
    let attributes = document.current_attributes();
    let with_attributes = |geometry: Geometry| Entity {
        geometry,
        layer: attributes.layer.clone(),
        style: attributes.style.clone(),
        color: attributes.color,
        ..Entity::default()
    };
    let mut entities = Vec::new();
    for row in &result.stations {
        // Control is already known - usually already in the drawing - so only
        // the stations this traverse computed become new points.
        if row.control {
            continue;
        }
        let mut point = with_attributes(Geometry::Point(PointGeometry {
            position: row.adjusted,
        }));
        point.properties.insert(
            String::from(point_name_property()),
            PropertyValue::Text(row.id.clone()),
        );
        entities.push(point);
    }
    let mut line = Polyline2 {
        vertices: result.stations.iter().map(|row| row.adjusted).collect(),
        // A loop comes back to its first station: said with the closed flag
        // rather than a repeated vertex, as everywhere in the drawing. A link
        // traverse closing on its own start is the same figure.
        closed: result.spec.kind == TraverseKind::ClosedLoop,
    };
    if result.stations.len() > 1
        && result.stations[result.stations.len() - 1].id == result.stations[0].id
    {
        line.vertices.pop();
        line.closed = true;
    }
    if line.vertices.len() >= 2 {
        let mut polyline = with_attributes(Geometry::Polyline(line));
        polyline.properties.insert(
            String::from("traverse"),
            PropertyValue::Text(result.spec.name.clone()),
        );
        entities.push(polyline);
    }
    let mut transaction = Transaction::new("SURVEY_TRAVERSE");
    transaction.add(create_entities(entities));
    Ok(Box::new(transaction))
}

/// A traverse field book, one leg per line: "station angle distance", blank
/// or comma separated; '#' starts a comment. Angles in any
/// parse_survey_angle format without blanks inside it (36d52m11.63s,
/// 36:52:11.63). ParseFailure naming the line and what is wrong with it;
/// InvalidArgument when there are no legs.
pub fn parse_traverse_legs(text: &str) -> Result<Vec<TraverseSetup>> {
    let mut legs = Vec::new();
    for (i, line) in lines_of(text).into_iter().enumerate() {
        let parts = fields(line);
        if parts.is_empty() {
            continue;
        }
        let context = line_context(i + 1, line);
        if parts.len() != 3 {
            return Err(make_error(
                ErrorCode::ParseFailure,
                "a leg is 'station angle distance' - three fields, the angle without blanks inside it (36d52m11.63s)",
                context,
            ));
        }
        let angle = parse_survey_angle(parts[1]).map_err(|error| {
            make_error(ErrorCode::ParseFailure, error.message, context.as_str())
        })?;
        let distance = parse_survey_number(parts[2], "the distance").map_err(|error| {
            make_error(ErrorCode::ParseFailure, error.message, context.as_str())
        })?;
        legs.push(TraverseSetup {
            station_id: String::from(parts[0]),
            angle,
            distance,
        });
    }
    if legs.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "no legs: one line per station, 'station angle distance'",
        ));
    }
    Ok(legs)
}

// ---- level book ------------------------------------------------------------------------

/// The common third-order allowance for a level run, 12 mm * sqrt(K km), in
/// metres per sqrt(km) - the figure katana_survey's leveling names for
/// allowable_level_misclosure. A default the user replaces with their
/// specification's; the report prints the k it used.
pub const THIRD_ORDER_LEVEL_ALLOWANCE: f64 = 0.012;

/// One line of a level book, in the height-of-collimation layout.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LevelBookLine {
    pub point: String,
    pub backsight: Option<f64>,
    pub intersight: Option<f64>,
    pub foresight: Option<f64>,
    /// On a foresight line only: the length levelled in the setup that ends
    /// there (backsight + foresight sight lengths), metres. Absent when not
    /// recorded - and then the allowable misclosure is not stated, rather than
    /// computed over a length of zero.
    pub distance: Option<f64>,
    pub line_number: usize, // 1-based line of the text it was read from
}

/// "point BS IS FS [distance]" per line, '-' for an empty column, blank or
/// comma separated, '#' starts a comment. A change point carries its
/// foresight and its backsight on one line. ParseFailure naming the line
/// otherwise.
pub fn parse_level_book(text: &str) -> Result<Vec<LevelBookLine>> {
    let mut book = Vec::new();
    for (i, line) in lines_of(text).into_iter().enumerate() {
        let parts = fields(line);
        if parts.is_empty() {
            continue;
        }
        let context = line_context(i + 1, line);
        if parts.len() != 4 && parts.len() != 5 {
            return Err(make_error(
                ErrorCode::ParseFailure,
                "a line is 'point BS IS FS [distance]', '-' for an empty column",
                context,
            ));
        }
        let reading = |index: usize, what: &str| -> Result<Option<f64>> {
            if parts[index] == "-" {
                return Ok(None);
            }
            parse_survey_number(parts[index], what)
                .map(Some)
                .map_err(|error| {
                    make_error(ErrorCode::ParseFailure, error.message, context.as_str())
                })
        };
        let mut book_line = LevelBookLine {
            point: String::from(parts[0]),
            line_number: i + 1,
            backsight: reading(1, "the backsight")?,
            intersight: reading(2, "the intersight")?,
            foresight: reading(3, "the foresight")?,
            distance: None,
        };
        if parts.len() == 5 {
            let distance = reading(4, "the distance")?;
            if let Some(length) = distance
                && !(length >= 0.0)
            {
                return Err(make_error(
                    ErrorCode::ParseFailure,
                    "a distance cannot be negative",
                    context,
                ));
            }
            book_line.distance = distance;
        }
        if book_line.backsight.is_none()
            && book_line.intersight.is_none()
            && book_line.foresight.is_none()
        {
            return Err(make_error(
                ErrorCode::ParseFailure,
                "the line has no reading",
                context,
            ));
        }
        book.push(book_line);
    }
    Ok(book)
}

#[derive(Clone, Debug, PartialEq)]
pub struct LevelBookSpec {
    pub name: String,
    /// Reduced level of the first line's point, the opening benchmark.
    pub start_level: f64,
    /// Known reduced level of the last line's point, when the run closes on a
    /// benchmark. Without it there is no misclosure and no adjustment.
    pub closing_level: Option<f64>,
    pub lines: Vec<LevelBookLine>,
    pub adjustment: LevelAdjustment,
    pub allowance_k: f64, // metres per sqrt(km)
}

impl Default for LevelBookSpec {
    fn default() -> Self {
        Self {
            name: String::from("Level run"),
            start_level: 0.0,
            closing_level: None,
            lines: Vec::new(),
            adjustment: LevelAdjustment::None,
            allowance_k: THIRD_ORDER_LEVEL_ALLOWANCE,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LevelBookRow {
    pub line: LevelBookLine,
    /// The instrument setup (1-based) the row's intersight or foresight was
    /// read from; for the opening backsight, 0.
    pub setup: usize,
    /// Height of collimation established on this row: the opening line and
    /// every change point carry one.
    pub height_of_collimation: Option<f64>,
    pub reduced_level: f64, // unadjusted
    pub correction: f64,
    pub adjusted_level: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LevelBookResult {
    pub spec: LevelBookSpec,
    pub rows: Vec<LevelBookRow>,            // one per line, in order
    pub run: katana_survey::LevelRunResult, // the survey library's own answer
    pub sum_backsights: f64,
    pub sum_foresights: f64,
    /// The arithmetic check of every level book: sum BS - sum FS must equal
    /// the last reduced level minus the first. Both sides are reported, not a
    /// verdict, because they are computed from the same readings by different
    /// routes and a difference in them is a booking error, not a misclosure.
    pub rise_minus_fall: f64, // sum_backsights - sum_foresights
    pub last_minus_first: f64,       // last unadjusted RL - start level
    pub misclosure: Option<f64>,     // computed - known closing level
    pub total_distance: Option<f64>, // absent unless every setup has one
    pub allowable: Option<f64>,      // allowance_k * sqrt(km), with the distance
}

/// katana_survey's compute_level_run over the book's setups, with the
/// intersights reduced from the height of collimation the library computed
/// for their setup and corrected by that setup's correction - the textbook
/// rule that every reading from one instrument position shares its
/// correction. The library does not model intersights, so this is the ONE
/// place they are reduced. InvalidArgument naming the line for a book that
/// does not start with a lone backsight, has a backsight away from a change
/// point, a reading row with no reading, or does not end on a foresight; the
/// library's errors otherwise (an adjustment without a closing level, ...).
pub fn compute_level_book(spec: &LevelBookSpec) -> Result<LevelBookResult> {
    let lines = &spec.lines;
    let refuse = |line: &LevelBookLine, message: &str| {
        make_error(
            ErrorCode::InvalidArgument,
            message,
            format!("line {} ({})", line.line_number, line.point),
        )
    };
    if lines.len() < 2 {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "a level book needs a backsight on the benchmark and a foresight",
        ));
    }
    // The shape of a height-of-collimation book, checked line by line so that
    // a booking error is named where it is rather than surfacing as a strange
    // reduced level further down.
    let opening = &lines[0];
    let Some(opening_backsight) = opening.backsight else {
        return Err(refuse(
            opening,
            "the book starts with a backsight, and only a backsight, on the benchmark",
        ));
    };
    if opening.intersight.is_some() || opening.foresight.is_some() {
        return Err(refuse(
            opening,
            "the book starts with a backsight, and only a backsight, on the benchmark",
        ));
    }
    if opening.distance.is_some() {
        return Err(refuse(
            opening,
            "a distance belongs on a foresight line, the end of its setup",
        ));
    }
    for line in &lines[1..] {
        if line.backsight.is_none() && line.intersight.is_none() && line.foresight.is_none() {
            return Err(refuse(line, "the line has no reading"));
        }
        if line.intersight.is_some() && (line.backsight.is_some() || line.foresight.is_some()) {
            return Err(refuse(line, "an intersight line carries no other reading"));
        }
        if line.backsight.is_some() && line.foresight.is_none() {
            return Err(refuse(
                line,
                "a backsight after the first must be on a change point, booked with that point's foresight",
            ));
        }
        if line.distance.is_some() && line.foresight.is_none() {
            return Err(refuse(
                line,
                "a distance belongs on a foresight line, the end of its setup",
            ));
        }
    }
    let last = &lines[lines.len() - 1];
    if last.foresight.is_none() || last.backsight.is_some() {
        return Err(refuse(
            last,
            "the book ends on a foresight; a backsight here starts a setup that is never finished",
        ));
    }

    // The setups the survey library levels: each runs from a backsight to the
    // next foresight. Intersights hang off the setup they were read from.
    let mut run = LevelRun {
        name: spec.name.clone(),
        start_elevation: spec.start_level,
        setups: Vec::new(),
        closing_elevation: spec.closing_level,
    };
    let mut setup_of_line = vec![0_usize; lines.len()]; // 0-based setup index
    let mut with_distance = 0_usize;
    let mut backsight = opening_backsight;
    for (i, line) in lines.iter().enumerate().skip(1) {
        setup_of_line[i] = run.setups.len();
        let Some(foresight) = line.foresight else {
            continue; // an intersight
        };
        run.setups.push(LevelSetup {
            foresight_point_id: line.point.clone(),
            backsight,
            foresight,
            distance: line.distance.unwrap_or(0.0),
        });
        if line.distance.is_some() {
            with_distance += 1;
        }
        if let Some(next) = line.backsight {
            backsight = next;
        }
    }
    // A length on some setups and not others would weight the adjustment by
    // lengths that are partly zero, and the allowable misclosure would be
    // computed over too short a line. Both are refused or left unstated
    // rather than computed from a zero nobody measured.
    let all_distances = with_distance == run.setups.len();
    if spec.adjustment == LevelAdjustment::ByDistance && !all_distances {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "adjustment by distance needs the length levelled on every foresight line",
            format!(
                "{} of {} setups have none",
                run.setups.len() - with_distance,
                run.setups.len()
            ),
        ));
    }
    let levelled = compute_level_run(&run, spec.adjustment)?;

    let mut result = LevelBookResult {
        spec: spec.clone(),
        run: levelled,
        ..LevelBookResult::default()
    };
    let points = &result.run.points;
    let mut rows = Vec::with_capacity(lines.len());
    for (i, line) in lines.iter().enumerate() {
        let mut row = LevelBookRow {
            line: line.clone(),
            ..LevelBookRow::default()
        };
        if i == 0 {
            row.height_of_collimation = Some(points[0].height_of_instrument);
            row.reduced_level = spec.start_level;
            row.adjusted_level = spec.start_level;
        } else {
            let setup = setup_of_line[i];
            let point = &points[setup];
            row.setup = setup + 1;
            if let Some(intersight) = line.intersight {
                // THE reduction the library does not do: the height of
                // collimation it computed for this setup, minus the staff
                // reading, corrected like every other reading of the setup.
                row.reduced_level = point.height_of_instrument - intersight;
                row.correction = point.correction;
                row.adjusted_level = row.reduced_level + row.correction;
            } else {
                row.reduced_level = point.elevation;
                row.correction = point.correction;
                row.adjusted_level = point.adjusted_elevation;
                if line.backsight.is_some() {
                    row.height_of_collimation = Some(points[setup + 1].height_of_instrument);
                }
            }
        }
        rows.push(row);
    }
    let last_elevation = points[points.len() - 1].elevation;
    result.rows = rows;
    result.sum_backsights = result.run.sum_backsights;
    result.sum_foresights = result.run.sum_foresights;
    result.rise_minus_fall = result.sum_backsights - result.sum_foresights;
    result.last_minus_first = last_elevation - spec.start_level;
    result.misclosure = result.run.misclosure;
    if all_distances {
        result.total_distance = Some(result.run.total_distance);
        result.allowable = Some(allowable_level_misclosure(
            spec.allowance_k,
            result.run.total_distance,
        )?);
    }
    Ok(result)
}

pub fn format_level_book_report(result: &LevelBookResult) -> String {
    let spec = &result.spec;
    let adjustment = match spec.adjustment {
        LevelAdjustment::BySetups => "misclosure shared equally between setups",
        LevelAdjustment::ByDistance => "misclosure shared by distance levelled",
        LevelAdjustment::None => "no adjustment",
    };
    let closing = match spec.closing_level {
        Some(level) => format!(", closing level {}", fixed(level, 3)),
        None => String::from(", no closing benchmark"),
    };
    let mut text = format!(
        "Level run '{}' - opening level {}{closing}, {adjustment}\n",
        spec.name,
        fixed(spec.start_level, 3)
    );
    let cell = |value: Option<f64>| value.map_or_else(String::new, |value| fixed(value, 3));
    text.push_str(&format!(
        "  {:<12} {:>8} {:>8} {:>8} {:>10} {:>10} {:>8} {:>10}\n",
        "Point", "BS", "IS", "FS", "HPC", "RL", "Corr", "Adj RL"
    ));
    for row in &result.rows {
        let corrected = spec.adjustment != LevelAdjustment::None && row.setup > 0;
        text.push_str(&format!(
            "  {:<12} {:>8} {:>8} {:>8} {:>10} {:>10} {:>8} {:>10}\n",
            row.line.point,
            cell(row.line.backsight),
            cell(row.line.intersight),
            cell(row.line.foresight),
            cell(row.height_of_collimation),
            fixed(row.reduced_level, 3),
            if corrected {
                signed_fixed(row.correction, 4)
            } else {
                String::new()
            },
            if corrected {
                fixed(row.adjusted_level, 3)
            } else {
                String::new()
            }
        ));
    }
    text.push_str(&format!(
        "  Check: sum BS {} - sum FS {} = {}; last RL - first RL = {}\n",
        fixed(result.sum_backsights, 3),
        fixed(result.sum_foresights, 3),
        signed_fixed(result.rise_minus_fall, 3),
        signed_fixed(result.last_minus_first, 3)
    ));
    match result.misclosure {
        Some(misclosure) => {
            text.push_str(&format!(
                "  Misclosure {} (computed - known)",
                signed_fixed(misclosure, 4)
            ));
            match (result.allowable, result.total_distance) {
                (Some(allowable), Some(total_distance)) => {
                    let within = misclosure.abs() <= allowable;
                    text.push_str(&format!(
                        "; allowable {} ({} mm x sqrt({} km)) - {}",
                        fixed(allowable, 4),
                        fixed(spec.allowance_k * 1000.0, 1),
                        fixed(total_distance / 1000.0, 3),
                        if within { "within it" } else { "EXCEEDS it" }
                    ));
                }
                _ => text
                    .push_str("; no allowable misclosure: not every setup has a length levelled"),
            }
        }
        None => text.push_str("  No closing benchmark: the run is not checked"),
    }
    text
}

// ---- angle calculator ------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum AngleInputUnit {
    /// DMS or decimal degrees, parse_survey_angle.
    #[default]
    Degrees,
    /// 400 to the circle.
    Gons,
    Radians,
    /// A quadrant bearing, N 45d30m E.
    Bearing,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AngleConversion {
    pub unit: AngleInputUnit,
    pub radians: f64, // as entered, sign and size kept
    pub degrees: f64,
    pub gons: f64,
    /// The same value read as a direction: wrapped into [0, 2*pi), its
    /// quadrant bearing and the reverse direction.
    pub azimuth: f64,
    pub back_azimuth: f64,
}

pub fn convert_angle(text: &str, unit: AngleInputUnit) -> Result<AngleConversion> {
    let radians = match unit {
        AngleInputUnit::Degrees => parse_survey_angle(text)?,
        AngleInputUnit::Gons => gon_to_radians(parse_survey_number(text, "an angle in gons")?),
        AngleInputUnit::Radians => parse_survey_number(text, "an angle in radians")?,
        AngleInputUnit::Bearing => parse_bearing(text)?,
    };
    let azimuth = normalize_azimuth(radians);
    Ok(AngleConversion {
        unit,
        radians,
        degrees: radians_to_degrees(radians),
        gons: radians_to_gon(radians),
        azimuth,
        back_azimuth: normalize_azimuth(azimuth + PI),
    })
}

pub fn format_angle_conversion(conversion: &AngleConversion) -> String {
    let mut text = format!(
        "Angle {}  =  {}\u{B0}  =  {} gon  =  {} rad\n",
        dms(conversion.radians),
        c_fixed(conversion.degrees, 9),
        c_fixed(conversion.gons, 8),
        c_fixed(conversion.radians, 10)
    );
    text.push_str(&format!(
        "  As a direction: azimuth {}   bearing {}   reverse {}",
        dms(conversion.azimuth),
        bearing(conversion.azimuth),
        dms(conversion.back_azimuth)
    ));
    text
}

// ---- coordinate converter --------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CoordinateConversionRow {
    pub label: String,
    /// As entered and as converted, each in its system's own terms: easting
    /// and northing for a projected system, latitude and longitude (decimal
    /// degrees) for a geographic one. Named so that a transposition shows.
    pub in_first: f64, // easting or latitude
    pub in_second: f64,  // northing or longitude
    pub out_first: f64,  // easting or latitude
    pub out_second: f64, // northing or longitude
    /// Grid scale factor and convergence at the point, for each side that is
    /// projected (geodesy's GridFactorCalculator).
    pub source_factors: Option<GridFactors>,
    pub target_factors: Option<GridFactors>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CoordinateConversion {
    pub source_name: String,
    pub target_name: String,
    pub source_geographic: bool,
    pub target_geographic: bool,
    pub operation: String,            // what PROJ applies, as it names it
    pub accuracy_metres: Option<f64>, // as the authority declares it
    pub rows: Vec<CoordinateConversionRow>,
}

fn crs_from_text(text: &str) -> Result<CoordinateReferenceSystem> {
    let body = trimmed(text);
    if body.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "name a coordinate system - an EPSG code such as 4326 or 28356",
        ));
    }
    if let Some(code) = parse_integer(body) {
        if !(1..=2_147_483_647).contains(&code) {
            return Err(make_error(ErrorCode::InvalidCrs, "not an EPSG code", body));
        }
        return CoordinateReferenceSystem::from_epsg(code as i32);
    }
    CoordinateReferenceSystem::from_user_input(body)
}

fn crs_title(crs: &CoordinateReferenceSystem) -> String {
    if crs.authority().is_empty() {
        return String::from(crs.name());
    }
    format!("{}:{} {}", crs.authority(), crs.code(), crs.name())
}

fn require_horizontal(crs: &CoordinateReferenceSystem) -> Status {
    if crs.is_geographic() || crs.is_projected() {
        return Ok(());
    }
    Err(make_error(
        ErrorCode::Unsupported,
        format!(
            "the converter works between geographic and projected systems; this is {}",
            kind_name(crs.kind())
        ),
        crs_title(crs),
    ))
}

/// How many of a converter line's label fields read as a coordinate of the
/// source system: an angle for a geographic one (parse_survey_angle, which
/// takes decimal degrees too, so "-33" and "-33:51:24.5" both count) and a
/// number for a projected one.
fn coordinate_like_fields(label: &[&str], geographic: bool) -> usize {
    label
        .iter()
        .filter(|field| {
            if geographic {
                parse_survey_angle(field).is_ok()
            } else {
                parse_finite_double(field).is_some()
            }
        })
        .count()
}

/// Converts typed points from one coordinate system to another through
/// geodesy's CoordinateTransformer - which REFUSES a ballpark (no datum
/// shift) operation, so a datum change it cannot do properly is an error and
/// not a point hundreds of metres out. `source` and `target` are an EPSG code
/// ("4326", "EPSG:4326") or anything PROJ accepts. `points` is one point per
/// line, "[label] first second", where first/second are easting and northing
/// for a projected system and LATITUDE and LONGITUDE for a geographic one
/// (decimal degrees or any parse_survey_angle format WITHOUT BLANKS INSIDE IT
/// (-33:51:24.5, -33d51m24.5s), since blanks separate the label and the two
/// coordinates; negative is south and west). The label is everything before
/// the last two fields and may hold blanks and one number ("102", "CP 1"); a
/// label holding two or more numbers (angles, for a geographic source) is
/// refused as an angle typed with blanks or a third coordinate, rather than
/// read as a point that was never meant. Heights are not converted. Errors:
/// InvalidCRS / NotFound / Unsupported from geodesy, ParseFailure naming the
/// line, InvalidArgument for no points.
pub fn convert_coordinates(
    source: &str,
    target: &str,
    points: &str,
) -> Result<CoordinateConversion> {
    let source_crs = crs_from_text(source)?;
    let target_crs = crs_from_text(target)?;
    require_horizontal(&source_crs)?;
    require_horizontal(&target_crs)?;
    let mut transformer =
        CoordinateTransformer::create(&source_crs, &target_crs, &TransformerOptions::default())?;

    let mut conversion = CoordinateConversion {
        source_name: crs_title(&source_crs),
        target_name: crs_title(&target_crs),
        source_geographic: source_crs.is_geographic(),
        target_geographic: target_crs.is_geographic(),
        operation: transformer.operation().name.clone(),
        accuracy_metres: transformer.operation().accuracy_metres,
        rows: Vec::new(),
    };

    let mut source_factors = if source_crs.is_projected() {
        Some(GridFactorCalculator::create(&source_crs)?)
    } else {
        None
    };
    let mut target_factors = if target_crs.is_projected() {
        Some(GridFactorCalculator::create(&target_crs)?)
    } else {
        None
    };

    for (i, line) in lines_of(points).into_iter().enumerate() {
        let parts = fields(line);
        if parts.is_empty() {
            continue;
        }
        let context = line_context(i + 1, line);
        if parts.len() < 2 {
            return Err(make_error(
                ErrorCode::ParseFailure,
                if conversion.source_geographic {
                    "a point is '[label] latitude longitude'"
                } else {
                    "a point is '[label] easting northing'"
                },
                context,
            ));
        }
        // Everything before the two coordinates is the label, so a point name
        // with a blank in it survives. But blanks also separate the fields of
        // a D M S angle typed as "-33 51 24.5", and taking the last two fields
        // as the coordinates then turns the rest into a label and reports a
        // point far from the one meant, without complaint. A point name holds
        // at most one number ("102", "CP 1"); a label with two or more is an
        // angle typed with blanks, or a coordinate too many (E N height).
        let label_fields = &parts[..parts.len() - 2];
        if coordinate_like_fields(label_fields, conversion.source_geographic) >= 2 {
            return Err(make_error(
                ErrorCode::ParseFailure,
                "two or more numbers before the last two fields: write an angle without blanks inside it (-33:51:24.5 or -33d51m24.5s) and give only the two coordinates, no height",
                context,
            ));
        }
        let mut row = CoordinateConversionRow {
            label: label_fields.join(" "),
            ..CoordinateConversionRow::default()
        };
        if row.label.is_empty() {
            row.label = format!("P{}", conversion.rows.len() + 1);
        }
        let first_text = parts[parts.len() - 2];
        let second_text = parts[parts.len() - 1];
        if conversion.source_geographic {
            let latitude = parse_survey_angle(first_text);
            let longitude = parse_survey_angle(second_text);
            match (latitude, longitude) {
                (Ok(latitude), Ok(longitude)) => {
                    row.in_first = radians_to_degrees(latitude);
                    row.in_second = radians_to_degrees(longitude);
                }
                (Err(error), _) | (Ok(_), Err(error)) => {
                    return Err(make_error(ErrorCode::ParseFailure, error.message, context));
                }
            }
        } else {
            let easting = parse_survey_number(first_text, "the easting");
            let northing = parse_survey_number(second_text, "the northing");
            match (easting, northing) {
                (Ok(easting), Ok(northing)) => {
                    row.in_first = easting;
                    row.in_second = northing;
                }
                (Err(error), _) | (Ok(_), Err(error)) => {
                    return Err(make_error(ErrorCode::ParseFailure, error.message, context));
                }
            }
        }

        // The generic Coordinate is traditional GIS order - x longitude or
        // easting, y latitude or northing (geodesy's coordinate.rs) - so the
        // latitude-first rows are swapped exactly here, and back below.
        let input = if conversion.source_geographic {
            Coordinate::new(row.in_second, row.in_first, 0.0)
        } else {
            Coordinate::new(row.in_first, row.in_second, 0.0)
        };
        let output = transformer.forward(input).map_err(|error| {
            make_error(
                error.code,
                error.message,
                format!("{context} {}", error.context),
            )
        })?;
        row.out_first = if conversion.target_geographic {
            output.y
        } else {
            output.x
        };
        row.out_second = if conversion.target_geographic {
            output.x
        } else {
            output.y
        };

        if let Some(calculator) = &mut source_factors {
            let factors = calculator
                .at(&ProjectedCoordinate::new(row.in_first, row.in_second, 0.0))
                .map_err(|error| make_error(error.code, error.message, context.as_str()))?;
            row.source_factors = Some(factors);
        }
        if let Some(calculator) = &mut target_factors {
            let factors = calculator
                .at(&ProjectedCoordinate::new(
                    row.out_first,
                    row.out_second,
                    0.0,
                ))
                .map_err(|error| make_error(error.code, error.message, context.as_str()))?;
            row.target_factors = Some(factors);
        }
        conversion.rows.push(row);
    }
    if conversion.rows.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "no points to convert: one per line, '[label] first second'",
        ));
    }
    Ok(conversion)
}

pub fn format_coordinate_conversion(conversion: &CoordinateConversion) -> String {
    let axes = |geographic: bool| {
        if geographic {
            "latitude, longitude"
        } else {
            "easting, northing"
        }
    };
    let value = |geographic: bool, number: f64| {
        // 1e-9 degree is 0.1 mm on the ground; 1 mm is the resolution every
        // other survey tool here prints a grid coordinate to.
        if geographic {
            c_fixed(number, 9)
        } else {
            fixed(number, 3)
        }
    };
    let factors = |grid: &GridFactors| {
        format!(
            "k {} conv {}",
            c_fixed(grid.point_scale_factor(), 8),
            dms(degrees_to_radians(grid.convergence_degrees))
        )
    };
    let mut text = format!(
        "Convert {} ({})  ->  {} ({})\n",
        conversion.source_name,
        axes(conversion.source_geographic),
        conversion.target_name,
        axes(conversion.target_geographic)
    );
    text.push_str(&format!("  Operation: {}", conversion.operation));
    match conversion.accuracy_metres {
        // std::format's "{:g}": six significant digits.
        Some(accuracy) => {
            text.push_str(&format!(" (accuracy {} m)\n", format_general(accuracy, 6)));
        }
        None => text.push_str(" (accuracy not stated)\n"),
    }
    for row in &conversion.rows {
        text.push_str(&format!(
            "  {}  {} {}  ->  {} {}",
            row.label,
            value(conversion.source_geographic, row.in_first),
            value(conversion.source_geographic, row.in_second),
            value(conversion.target_geographic, row.out_first),
            value(conversion.target_geographic, row.out_second)
        ));
        if let Some(source) = &row.source_factors {
            text.push_str(&format!("   source {}", factors(source)));
        }
        if let Some(target) = &row.target_factors {
            text.push_str(&format!("   target {}", factors(target)));
        }
        text.push('\n');
    }
    text.push_str("  Heights are not converted, and nothing in the drawing is moved");
    text
}

/// The selected point entities as convert_coordinates lines for `source`:
/// "name easting northing", with '#', ',' - and the blanks of a name holding
/// two numbers, "STN 3 4" - turned into '_' so the name reads back as one
/// label. The drawing's own x and y are easting and northing, so a geographic
/// source system is refused (InvalidArgument) rather than having its latitude
/// filled with an easting. NotFound / InvalidCRS from geodesy.
pub fn conversion_lines_for_selection(document: &Document, source: &str) -> Result<String> {
    let crs = crs_from_text(source)?;
    if crs.is_geographic() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "the drawing's points are easting and northing; choose the projected system they are in as the source",
            crs_title(&crs),
        ));
    }
    let mut lines = String::new();
    for position in selected_point_positions(document) {
        let mut label = if position.name.is_empty() {
            format!("id{}", position.entity.unwrap_or_default())
        } else {
            position.name.clone()
        };
        // '#' starts a comment and ',' separates fields in the lines this
        // makes; a name carrying either would come back cut short.
        label = label.replace(['#', ','], "_");
        // Nor may it hold two numbers apart ("STN 3 4"): convert_coordinates
        // refuses that as an angle typed with blanks. Joined, it is one field.
        if coordinate_like_fields(&fields(&label), crs.is_geographic()) >= 2 {
            label = label
                .chars()
                .map(|character| if is_space(character) { '_' } else { character })
                .collect();
        }
        lines.push_str(&format!(
            "{label} {} {}\n",
            format_exact_real(position.point.x),
            format_exact_real(position.point.y)
        ));
    }
    if lines.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidState,
            "no point entities are selected",
        ));
    }
    Ok(lines)
}
