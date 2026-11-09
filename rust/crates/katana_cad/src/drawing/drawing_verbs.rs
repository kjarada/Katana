//! The drawing system's verbs of the command line (docs/drawing.md, "The
//! command line"; src/katana_cad/drawing/drawing_verbs.cpp):
//! CommandInterpreter's members for the VERTEX verbs and the whole-polyline
//! edits (WEED, DENSIFY, STRAIGHTEN, CLOSE, OPEN, STARTVERTEX, VERTEXZ), the
//! draw verbs (PLINE with ARC and LINE, PLINE3D, SPLINE, ELLIPSE, XLINE, RAY,
//! DLINE) and the drafting settings (ORTHO, POLAR, TRACKING, ANGLES, LOCK,
//! SNAP, DRAFTING). Kept here, with the rest of the drawing system, rather
//! than in command_interpreter.rs.
//!
//! Written for an agent as much as for a person, as the annotation verbs are:
//!
//! * OPTIONS ARE key=value, in any order after the positional arguments, keys
//!   case-insensitive; an unknown key is refused naming the known ones.
//! * REPLIES ARE RECORDS: `key=value` pairs separated by spaces, one record
//!   per line; numbers read back exactly; what was made says its id.
//! * EVERY EDIT IS ONE STEP through Document::execute, built by the same
//!   functions the grips, the tools and the Vertices panel use
//!   (polyline_vertices, vertex_editing.rs, vertex_table.rs, draw_shapes.rs),
//!   so a vertex moved by a verb and one dragged by a grip are the same edit.
//!   The drafting settings are not drawing edits: they change at once and are
//!   not on the undo stack.
//!
//! Vertex indices count from 0, as the Vertices panel's first column does.

use std::collections::BTreeMap;

use katana_commands::CommandPtr;
use katana_core::error::make_error;
use katana_core::text::{format_exact_real, parse_finite_double, parse_integer};
use katana_core::{Error, ErrorCode, Result, Status};
use katana_entity::{Entity, EntityId, EnumName, Geometry};
use katana_geometry::polyline_vertices::{
    PolylineResult, change_start_vertex, close_polyline, delete_vertices, densify, grade_between,
    insert_vertex, interpolate_heights, move_vertex, nearest_segment, open_polyline,
    set_vertex_height, straighten, weed,
};
use katana_geometry::{CurvePolyline2, CurveVertex, Ellipse2, Point2, Polyline2, Spline2, Vec2};
use katana_math::numerics::{normalize_angle, tolerance};
use katana_math::{DEG_TO_RAD, RAD_TO_DEG};

use crate::command_interpreter::CommandInterpreter;
use crate::document::Document;
use crate::drawing::drafting::{
    AngleConvention, DraftingSettings, PrecisePoint, parse_direction, parse_precise_point,
};
use crate::drawing::draw_shapes::{
    construction_segment, create_construction, create_drawn, double_line_sides, drawn_entity,
    tangent_bulge,
};
use crate::drawing::vertex_editing::{
    SURVEY_POINT_TOLERANCE, edit_each_polyline, edit_polyline, edit_polylines, is_polyline_entity,
    read_polyline, vertex_range, vertices_on_survey_points, write_polyline,
};
use crate::drawing::vertex_table::{
    VertexColumn, VertexRow, edit_vertex_cell, vertex_column_from_string, vertex_rows,
};
use crate::snapping::{ALL_SNAP_MODES, SnapMode, SnapModes};
use crate::utilities::utility_data::trimmed_text;

fn upper_of(text: &str) -> String {
    text.to_ascii_uppercase()
}

fn lower_of(text: &str) -> String {
    text.to_ascii_lowercase()
}

fn usage_error(text: &str) -> Error {
    Error::new(ErrorCode::InvalidArgument, format!("usage: {text}"))
}

// The verbs' own short names, mapped to the name each is handled under.
const DRAWING_ALIASES: [(&str, &str); 9] = [
    ("3DPOLY", "PLINE3D"),
    ("3DPOLYLINE", "PLINE3D"),
    ("SPL", "SPLINE"),
    ("EL", "ELLIPSE"),
    ("XL", "XLINE"),
    ("DL", "DLINE"),
    ("VERTICES", "VERTEX"),
    ("SIMPLIFY", "WEED"),
    ("OSNAP", "SNAP"),
];

fn canonical(verb: &str) -> &str {
    DRAWING_ALIASES
        .iter()
        .find(|(alias, _)| *alias == verb)
        .map_or(verb, |&(_, name)| name)
}

// ---- options -------------------------------------------------------------------------

fn is_option_key(key: &str) -> bool {
    !key.is_empty() && key.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
}

fn is_option_token(token: &str) -> bool {
    token
        .find('=')
        .is_some_and(|equals| equals > 0 && is_option_key(&token[..equals]))
}

#[derive(Clone, Debug, Default)]
struct Arguments {
    positional: Vec<String>,
    // In the order given, since VERTEX SET applies its fields in turn.
    options: Vec<(String, String)>,
}

impl Arguments {
    fn find(&self, key: &str) -> Option<&String> {
        self.options
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value)
    }
}

fn split_arguments(args: &[String], from: usize, known: &[&str]) -> Result<Arguments> {
    let mut result = Arguments::default();
    for token in args.get(from..).unwrap_or_default() {
        if !is_option_token(token) {
            result.positional.push(token.clone());
            continue;
        }
        let equals = token.find('=').unwrap_or(token.len());
        let key = lower_of(&token[..equals]);
        if !known.contains(&key.as_str()) {
            let list = if known.is_empty() {
                String::from("none")
            } else {
                known.join(" ")
            };
            return Err(make_error(
                ErrorCode::InvalidArgument,
                format!("unknown option '{key}'"),
                format!("known: {list}"),
            ));
        }
        result
            .options
            .push((key, String::from(&token[equals + 1..])));
    }
    Ok(result)
}

fn number_of(text: &str, what: &str) -> Result<f64> {
    parse_finite_double(trimmed_text(text)).ok_or_else(|| {
        make_error(
            ErrorCode::ParseFailure,
            format!("expected a number for {what}"),
            text,
        )
    })
}

fn positive_of(text: &str, what: &str) -> Result<f64> {
    let value = number_of(text, what)?;
    if !(value > 0.0) {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!("{what} must be greater than zero"),
            text,
        ));
    }
    Ok(value)
}

fn switch_of(text: &str, what: &str) -> Result<bool> {
    match lower_of(text).as_str() {
        "on" | "yes" | "true" | "1" => Ok(true),
        "off" | "no" | "false" | "0" => Ok(false),
        _ => Err(make_error(
            ErrorCode::ParseFailure,
            format!("expected on or off for {what}"),
            text,
        )),
    }
}

fn index_of(text: &str, what: &str) -> Result<usize> {
    match parse_integer(trimmed_text(text)) {
        Some(value) if value >= 0 => Ok(value as usize),
        _ => Err(make_error(
            ErrorCode::ParseFailure,
            format!("expected a vertex index (0 or more) for {what}"),
            text,
        )),
    }
}

fn entity_id_of(text: &str) -> Result<EntityId> {
    let body = trimmed_text(text);
    let body = body.strip_prefix('#').unwrap_or(body);
    match parse_integer(body) {
        Some(value) if value > 0 => Ok(value as EntityId),
        _ => Err(make_error(
            ErrorCode::ParseFailure,
            "expected an entity id (12 or #12)",
            text,
        )),
    }
}

// ---- replies ---------------------------------------------------------------------------

fn real(value: f64) -> String {
    format_exact_real(value)
}

fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}

fn height_text(height: Option<f64>) -> String {
    height.map_or_else(|| String::from("none"), real)
}

// Whole-circle bearing in decimal degrees, clockwise from north, 0 to 360.
fn bearing_degrees(radians: f64) -> String {
    let mut degrees = 90.0 - radians * RAD_TO_DEG;
    degrees %= 360.0;
    if degrees < 0.0 {
        degrees += 360.0;
    }
    real(degrees)
}

fn kind_text(entity: &Entity) -> String {
    lower_of(entity.r#type().name())
}

// The summary record of polyline `id` as it now stands.
fn polyline_record(document: &Document, id: EntityId) -> String {
    let Some(entity) = document.model().entities.find(id) else {
        return format!("id={id} deleted=yes");
    };
    let Some(polyline) = read_polyline(entity) else {
        return format!("id={id} kind={}", kind_text(entity));
    };
    let heights = polyline
        .vertices
        .iter()
        .filter(|vertex| vertex.height.is_some())
        .count();
    format!(
        "id={id} kind={} closed={} vertices={} arcs={} heights={heights} length={}",
        kind_text(entity),
        if polyline.closed { "yes" } else { "no" },
        polyline.vertices.len(),
        if polyline.has_arcs() { "yes" } else { "no" },
        real(polyline.length())
    )
}

fn vertex_record(row: &VertexRow) -> String {
    format!(
        "index={} x={} y={} z={} bulge={} bearing={} distance={}",
        row.index,
        real(row.easting),
        real(row.northing),
        height_text(row.height),
        real(row.bulge),
        row.bearing
            .map_or_else(|| String::from("none"), bearing_degrees),
        row.distance.map_or_else(|| String::from("none"), real)
    )
}

// The snap modes in bit order, Endpoint (1) to MidBetween (1 << 14): the C++
// casts each bit to the enumeration.
const SNAP_MODES: [SnapMode; 15] = [
    SnapMode::Endpoint,
    SnapMode::Midpoint,
    SnapMode::Center,
    SnapMode::Intersection,
    SnapMode::Perpendicular,
    SnapMode::Tangent,
    SnapMode::Nearest,
    SnapMode::Grid,
    SnapMode::Quadrant,
    SnapMode::Node,
    SnapMode::Extension,
    SnapMode::Parallel,
    SnapMode::ApparentIntersection,
    SnapMode::From,
    SnapMode::MidBetween,
];

// As modes_of reads it: one word, "apparentintersection" - a blank inside a
// value would end it for every key=value reader.
fn compact_name(mode: SnapMode) -> String {
    lower_of(mode.name()).replace(' ', "")
}

fn mode_names(modes: SnapModes) -> String {
    let names: Vec<String> = SNAP_MODES
        .iter()
        .filter(|&&mode| modes & mode as SnapModes != 0)
        .map(|&mode| compact_name(mode))
        .collect();
    if names.is_empty() {
        String::from("none")
    } else {
        names.join(",")
    }
}

fn modes_of(text: &str) -> Result<SnapModes> {
    let lower = lower_of(text);
    if lower == "none" || lower.is_empty() {
        return Ok(0);
    }
    if lower == "all" {
        return Ok(ALL_SNAP_MODES);
    }
    let mut modes: SnapModes = 0;
    for name in lower.split(',') {
        let mut found = false;
        // Every mode mode_names writes, so a record reads back.
        for &mode in &SNAP_MODES {
            let known = lower_of(mode.name());
            if name == known
                || name == compact_name(mode)
                || (name == "centre" && known == "center")
            {
                modes |= mode as SnapModes;
                found = true;
            }
        }
        if !found {
            return Err(make_error(
                ErrorCode::ParseFailure,
                format!("unknown snap mode '{name}'"),
                "known: endpoint midpoint center intersection perpendicular tangent nearest grid \
                 quadrant node extension parallel apparentintersection, all, none",
            ));
        }
    }
    Ok(modes)
}

// An angle held in radians, in degrees to a nanodegree: typed as 15, it is
// held as 15 x pi / 180 and would come back as 14.999999999999998.
fn degrees(radians: f64) -> String {
    real((radians * RAD_TO_DEG * 1.0e9).round() / 1.0e9)
}

fn drafting_record(s: &DraftingSettings) -> String {
    format!(
        "ortho={} polar={} increment={} tracking={} angles={} anglelock={} lengthlock={} snap={} \
         modes={}",
        on_off(s.ortho),
        on_off(s.polar),
        degrees(s.polar_increment),
        on_off(s.object_tracking),
        if s.angles == AngleConvention::Bearing {
            "bearing"
        } else {
            "ccw"
        },
        s.angle_lock.map_or_else(|| String::from("none"), degrees),
        s.length_lock.map_or_else(|| String::from("none"), real),
        on_off(s.snap_enabled),
        mode_names(s.snap_modes)
    )
}

// ---- helpers shared by the verbs ----------------------------------------------------------

// The polylines a target names: ids, or SELECTION (every polyline selected).
fn targets_of(
    document: &Document,
    tokens: &[String],
    empty_is_selection: bool,
) -> Result<Vec<EntityId>> {
    let model = document.model();
    let selection = (tokens.is_empty() && empty_is_selection)
        || (tokens.len() == 1 && upper_of(&tokens[0]) == "SELECTION");
    if selection {
        let ids: Vec<EntityId> = document
            .selection()
            .ids()
            .into_iter()
            .filter(|&id| model.entities.find(id).is_some_and(is_polyline_entity))
            .collect();
        if ids.is_empty() {
            return Err(Error::new(
                ErrorCode::InvalidState,
                "no polyline is selected",
            ));
        }
        return Ok(ids);
    }
    if tokens.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "name the polyline: an id or SELECTION",
        ));
    }
    let mut ids = Vec::new();
    for token in tokens {
        let id = entity_id_of(token)?;
        let Some(entity) = model.entities.find(id) else {
            return Err(make_error(
                ErrorCode::NotFound,
                "entity does not exist",
                token.as_str(),
            ));
        };
        if !is_polyline_entity(entity) {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                format!("entity {id} is a {}, not a polyline", kind_text(entity)),
            ));
        }
        ids.push(id);
    }
    Ok(ids)
}

fn polyline_of(document: &Document, id: EntityId) -> Result<CurvePolyline2> {
    let Some(entity) = document.model().entities.find(id) else {
        return Err(make_error(
            ErrorCode::NotFound,
            "entity does not exist",
            format!("{id}"),
        ));
    };
    read_polyline(entity).ok_or_else(|| {
        Error::new(
            ErrorCode::InvalidArgument,
            format!("entity {id} is a {}, not a polyline", kind_text(entity)),
        )
    })
}

fn check_index(polyline: &CurvePolyline2, index: usize) -> Status {
    let count = polyline.vertices.len();
    if index >= count {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!(
                "vertex {index} does not exist; the polyline has {count} (0 to {})",
                count.wrapping_sub(1)
            ),
        ));
    }
    Ok(())
}

// A vertex index of `polyline`, read and checked.
fn index_in(polyline: &CurvePolyline2, text: &str, verb: &str) -> Result<usize> {
    let index = index_of(text, verb)?;
    check_index(polyline, index)?;
    Ok(index)
}

// Runs `command` and replies with one record per polyline edited.
fn run_records(
    document: &mut Document,
    ids: &[EntityId],
    command: CommandPtr,
    extra: &str,
) -> Result<String> {
    document.execute(command)?;
    let records: Vec<String> = ids
        .iter()
        .map(|&id| polyline_record(document, id) + extra)
        .collect();
    Ok(records.join("\n"))
}

// Runs `command` and replies with a record for each entity it made.
fn made(document: &mut Document, command: CommandPtr) -> Result<String> {
    document.execute(command)?;
    let mut records: Vec<String> = Vec::new();
    for id in document.last_created_entities() {
        let Some(entity) = document.model().entities.find(id) else {
            continue;
        };
        records.push(if is_polyline_entity(entity) {
            polyline_record(document, id)
        } else {
            format!("id={id} kind={} layer={}", kind_text(entity), entity.layer)
        });
    }
    Ok(records.join("\n"))
}

// ---- the verbs ------------------------------------------------------------------------

impl CommandInterpreter {
    pub(crate) fn is_drawing_verb(verb: &str, args: &[String]) -> bool {
        const VERBS: [&str; 20] = [
            "VERTEX",
            "WEED",
            "DENSIFY",
            "STRAIGHTEN",
            "CLOSE",
            "STARTVERTEX",
            "VERTEXZ",
            "PLINE3D",
            "SPLINE",
            "ELLIPSE",
            "XLINE",
            "RAY",
            "DLINE",
            "ORTHO",
            "POLAR",
            "TRACKING",
            "ANGLES",
            "LOCK",
            "SNAP",
            "DRAFTING",
        ];
        let name = canonical(verb);
        if VERBS.contains(&name) {
            return true;
        }
        if name == "PLINE" {
            // Every PLINE, the plain PLINE p p [CLOSE] included, so that each
            // replies with the id of what it made.
            return true;
        }
        if name == "OPEN" {
            // OPEN directory opens a project; OPEN with no argument,
            // SELECTION or #ids opens polylines.
            return args
                .iter()
                .all(|token| upper_of(token) == "SELECTION" || token.starts_with('#'));
        }
        false
    }

    pub(crate) fn drawing_verb(
        &mut self,
        document: &mut Document,
        verb: &str,
        args: &[String],
    ) -> Result<String> {
        let name = canonical(verb);
        match name {
            "VERTEX" => self.vertex_verb(document, args),
            "WEED" | "DENSIFY" | "STRAIGHTEN" | "CLOSE" | "OPEN" | "STARTVERTEX" | "VERTEXZ" => {
                polyline_verb(document, name, args)
            }
            "ORTHO" | "POLAR" | "TRACKING" | "ANGLES" | "LOCK" | "SNAP" | "DRAFTING" => {
                drafting_verb(document, name, args)
            }
            _ => self.draw_shape_verb(document, name, args),
        }
    }

    pub(crate) fn drawing_help_text() -> String {
        String::from(DRAWING_HELP_TEXT)
    }

    /// A point with an optional height, relative input measured from `from`
    /// when given (a vertex being moved) or else from the last point.
    pub(crate) fn parse_drawing_point(
        &mut self,
        document: &Document,
        text: &str,
        from: Option<Point2>,
    ) -> Result<PrecisePoint> {
        let point = parse_precise_point(text, from.or(self.last_point), document.drafting())?;
        self.last_point = Some(point.point);
        Ok(point)
    }

    // ---- VERTEX ---------------------------------------------------------------------------

    fn vertex_verb(&mut self, document: &mut Document, args: &[String]) -> Result<String> {
        if args.len() < 2 {
            return Err(usage_error("VERTEX LIST|INSERT|DELETE|MOVE|SET id ..."));
        }
        let sub = upper_of(&args[0]);
        if !matches!(sub.as_str(), "LIST" | "INSERT" | "DELETE" | "MOVE" | "SET") {
            return Err(make_error(
                ErrorCode::ParseFailure,
                "unknown VERTEX action; LIST INSERT DELETE MOVE SET",
                args[0].as_str(),
            ));
        }
        let id = entity_id_of(&args[1])?;
        let polyline = polyline_of(document, id)?;

        match sub.as_str() {
            "LIST" => {
                if args.len() != 2 {
                    return Err(usage_error("VERTEX LIST id"));
                }
                let mut text = polyline_record(document, id);
                for row in vertex_rows(&polyline) {
                    text += &format!("\n{}", vertex_record(&row));
                }
                Ok(text)
            }
            "INSERT" => {
                let parsed = split_arguments(args, 2, &["after"])?;
                if parsed.positional.len() != 1 {
                    return Err(usage_error(
                        "VERTEX INSERT id x,y[,z] | #id@x,y[,z] | #id.sN [after=N]",
                    ));
                }
                // A point, or a point OF an entity (parse_anchored_point):
                // "#12@x,y" is the place on #12 nearest x,y - ON the line, as
                // the window's Insert Vertex puts a click - and "#12.s3"
                // segment 3's middle. On this polyline the anchor names its
                // segment itself, where nearest_segment could take a neighbour
                // at a vertex.
                let given = &parsed.positional[0];
                let place;
                let mut z: Option<f64> = None;
                let mut anchored_segment: Option<usize> = None;
                if given.starts_with('#') {
                    // "#12@x,y,z": on the line, with z its height, as x,y,z
                    // typed at the window's Insert Vertex is. The anchor reads
                    // x,y alone.
                    let mut anchor_text = given.clone();
                    if let Some(at) = given.find('@') {
                        let near = &given[at + 1..];
                        if near.matches(',').count() == 2 {
                            let comma = near.rfind(',').unwrap_or(near.len());
                            z = Some(number_of(&near[comma + 1..], "the height")?);
                            anchor_text = String::from(&given[..at + 1 + comma]);
                        }
                    }
                    let anchored = self.parse_anchored_point(document, &anchor_text)?;
                    place = anchored.point;
                    if anchored.r#ref.entity == id
                        && (anchored.r#ref.point == katana_entity::AnchorPoint::Along
                            || anchored.r#ref.point == katana_entity::AnchorPoint::SegmentMid)
                    {
                        anchored_segment = Some(anchored.r#ref.index as usize);
                    }
                } else {
                    let at = self.parse_drawing_point(document, given, None)?;
                    place = at.point;
                    z = at.z;
                }
                let mut segment = 0;
                if let Some(after) = parsed.find("after") {
                    let index = index_of(after, "after")?;
                    let segments = polyline
                        .vertices
                        .len()
                        .wrapping_sub(usize::from(!polyline.closed));
                    if index >= segments {
                        return Err(Error::new(
                            ErrorCode::InvalidArgument,
                            format!("there is no segment after vertex {index}"),
                        ));
                    }
                    // A point of this polyline names its segment itself: an
                    // after= naming another put a point of segment 0 after
                    // vertex 1, and the polyline folded back on itself.
                    if let Some(anchored) = anchored_segment
                        && anchored != index
                    {
                        return Err(Error::new(
                            ErrorCode::InvalidArgument,
                            format!(
                                "the point {given} is on segment {anchored}, not after vertex \
                                 {index}; give one or the other"
                            ),
                        ));
                    }
                    segment = index;
                } else if let Some(anchored) = anchored_segment {
                    segment = anchored;
                } else if let Some(nearest) = nearest_segment(&polyline, place) {
                    segment = nearest;
                }
                document.execute(edit_polyline(id, "VERTEX_INSERT", move |p| {
                    insert_vertex(p, segment, place, z)
                }))?;
                Ok(format!(
                    "{} inserted={}",
                    polyline_record(document, id),
                    segment + 1
                ))
            }
            "DELETE" => {
                if args.len() < 3 {
                    return Err(usage_error("VERTEX DELETE id N [N...]"));
                }
                let mut indices = Vec::new();
                for word in &args[2..] {
                    let index = index_of(word, "DELETE")?;
                    check_index(&polyline, index)?;
                    indices.push(index);
                }
                let count = indices.len();
                document.execute(edit_polyline(id, "VERTEX_DELETE", move |p| {
                    delete_vertices(p, indices.clone())
                }))?;
                Ok(format!("{} deleted={count}", polyline_record(document, id)))
            }
            "MOVE" => {
                if args.len() != 4 {
                    return Err(usage_error(
                        "VERTEX MOVE id N x,y[,z] | @dx,dy[,dz] | @dist<dir",
                    ));
                }
                let at = index_of(&args[2], "MOVE")?;
                check_index(&polyline, at)?;
                let target = self.parse_drawing_point(
                    document,
                    &args[3],
                    Some(polyline.vertices[at].position),
                )?;
                let base = polyline.vertices[at].height;
                let relative = args[3].starts_with('@');
                document.execute(edit_polyline(id, "VERTEX_MOVE", move |p| {
                    let moved = move_vertex(p, at, target.point)?;
                    let Some(z) = target.z else {
                        return Ok(moved);
                    };
                    // A relative dz is a change of height; an absolute z is
                    // the height.
                    let height = if relative {
                        let Some(base) = base else {
                            return Err(Error::new(
                                ErrorCode::InvalidArgument,
                                "the vertex has no height for a dz to change; give z absolutely",
                            ));
                        };
                        base + z
                    } else {
                        z
                    };
                    set_vertex_height(&moved, at, Some(height))
                }))?;
                let after = polyline_of(document, id)?;
                Ok(format!(
                    "{}\n{}",
                    polyline_record(document, id),
                    vertex_record(&vertex_rows(&after)[at])
                ))
            }
            _ => {
                // SET
                let parsed = split_arguments(
                    args,
                    2,
                    &[
                        "x", "y", "e", "n", "easting", "northing", "z", "height", "bulge",
                        "bearing", "distance",
                    ],
                )?;
                if parsed.positional.len() != 1 || parsed.options.is_empty() {
                    return Err(usage_error(
                        "VERTEX SET id N [x= y= z= bulge= bearing= distance=]",
                    ));
                }
                let at = index_of(&parsed.positional[0], "SET")?;
                check_index(&polyline, at)?;
                let mut cells: Vec<(VertexColumn, String)> = Vec::new();
                for (key, value) in &parsed.options {
                    let name = if key == "z" { "height" } else { key.as_str() };
                    let Some(column) = vertex_column_from_string(name) else {
                        return Err(Error::new(
                            ErrorCode::InvalidArgument,
                            format!("unknown column '{key}'"),
                        ));
                    };
                    cells.push((column, value.clone()));
                }
                document.execute(edit_polyline(id, "VERTEX_SET", move |p| {
                    let mut current = p.clone();
                    for (column, text) in &cells {
                        current = edit_vertex_cell(
                            &current,
                            at,
                            *column,
                            text,
                            AngleConvention::Bearing,
                        )?;
                    }
                    Ok(current)
                }))?;
                let after = polyline_of(document, id)?;
                Ok(format!(
                    "{}\n{}",
                    polyline_record(document, id),
                    vertex_record(&vertex_rows(&after)[at])
                ))
            }
        }
    }

    // ---- the draw verbs ---------------------------------------------------------------

    fn draw_shape_verb(
        &mut self,
        document: &mut Document,
        verb: &str,
        args: &[String],
    ) -> Result<String> {
        let attributes = document.current_attributes();

        if verb == "PLINE" || verb == "PLINE3D" {
            let three_d = verb == "PLINE3D";
            let parsed = if three_d {
                split_arguments(args, 0, &["z"])?
            } else {
                split_arguments(args, 0, &[])?
            };
            let default_z = match parsed.find("z") {
                Some(z) => Some(number_of(z, "z")?),
                None => None,
            };
            let mut path = CurvePolyline2::default();
            let mut arc = false;
            let mut closed = false;
            let tokens = &parsed.positional;
            for (i, token) in tokens.iter().enumerate() {
                let word = upper_of(token);
                if !three_d && (word == "ARC" || word == "A") {
                    arc = true;
                    continue;
                }
                if !three_d && (word == "LINE" || word == "L") {
                    arc = false;
                    continue;
                }
                if (word == "CLOSE" || word == "C") && i + 1 == tokens.len() {
                    closed = true;
                    continue;
                }
                let point = self.parse_drawing_point(document, token, None)?;
                if let Some(last) = path.vertices.last()
                    && point.point.distance_to(last.position) <= tolerance::GEOMETRIC
                {
                    return Err(make_error(
                        ErrorCode::InvalidGeometry,
                        "two points in a row are the same point",
                        token.as_str(),
                    ));
                }
                if arc && !path.vertices.is_empty() {
                    let bulge = tangent_bulge(&path, point.point);
                    if let Some(last) = path.vertices.last_mut() {
                        last.bulge = bulge;
                    }
                }
                path.vertices.push(CurveVertex {
                    position: point.point,
                    bulge: 0.0,
                    height: point.z.or(default_z),
                });
            }
            if path.vertices.len() < 2 || (closed && path.vertices.len() < 3) {
                return Err(usage_error(if three_d {
                    "PLINE3D p p [p...] [CLOSE] [z=]"
                } else {
                    "PLINE p p [ARC p...] [LINE p...] [CLOSE]"
                }));
            }
            if closed {
                path.closed = true;
                // In arc mode the closing segment is a tangent arc too, as the
                // Polyline tool closes.
                let bulge = if arc {
                    tangent_bulge(&path, path.vertices[0].position)
                } else {
                    0.0
                };
                if let Some(last) = path.vertices.last_mut() {
                    last.bulge = bulge;
                }
            }
            let entity = write_polyline(drawn_entity(Geometry::default(), &attributes), &path)?;
            return made(document, create_drawn("CREATE_POLYLINE", vec![entity]));
        }

        if verb == "SPLINE" {
            let parsed = split_arguments(args, 0, &["control", "degree"])?;
            let control = match parsed.find("control") {
                Some(text) => switch_of(text, "control")?,
                None => false,
            };
            let mut degree = 3;
            if let Some(text) = parsed.find("degree") {
                match parse_integer(text) {
                    Some(value) if (1..=10).contains(&value) => degree = value as i32,
                    _ => {
                        return Err(make_error(
                            ErrorCode::InvalidArgument,
                            "degree must be from 1 to 10",
                            text.as_str(),
                        ));
                    }
                }
            }
            let mut points = Vec::new();
            for token in &parsed.positional {
                points.push(self.parse_drawing_point(document, token, None)?.point);
            }
            if points.len() < 2 {
                return Err(usage_error("SPLINE p p [p...] [control=on|off] [degree=3]"));
            }
            let spline = if control {
                Spline2::from_control_points(points, degree)?
            } else {
                Spline2::through_points(points, degree)?
            };
            return made(
                document,
                create_drawn(
                    "CREATE_SPLINE",
                    vec![drawn_entity(Geometry::Spline(spline), &attributes)],
                ),
            );
        }

        if verb == "ELLIPSE" {
            let parsed = split_arguments(args, 0, &["minor", "ratio", "start", "end"])?;
            if parsed.positional.len() != 2 {
                return Err(usage_error(
                    "ELLIPSE centre axis-end minor=|ratio= [start=deg end=deg]",
                ));
            }
            let centre = self.parse_drawing_point(document, &parsed.positional[0], None)?;
            let axis_end = self.parse_drawing_point(document, &parsed.positional[1], None)?;
            let major = centre.point.distance_to(axis_end.point);
            let other = if let Some(minor) = parsed.find("minor") {
                positive_of(minor, "minor")?
            } else if let Some(ratio) = parsed.find("ratio") {
                positive_of(ratio, "ratio")? * major
            } else {
                return Err(usage_error(
                    "ELLIPSE centre axis-end minor=|ratio= [start=deg end=deg]",
                ));
            };
            let Some(mut shape) = Ellipse2::from_axes(centre.point, axis_end.point, other) else {
                return Err(Error::new(
                    ErrorCode::InvalidGeometry,
                    "the ellipse's axes need lengths",
                ));
            };
            let start = parsed.find("start");
            let end = parsed.find("end");
            if start.is_none() != end.is_none() {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    "an elliptical arc needs start= and end=",
                ));
            }
            if let (Some(start), Some(end)) = (start, end) {
                let a = number_of(start, "start");
                let b = number_of(end, "end");
                let a = a?;
                let b = b?;
                let from = a * DEG_TO_RAD;
                let sweep = normalize_angle(b * DEG_TO_RAD - from);
                if !(sweep > tolerance::ANGULAR) {
                    return Err(Error::new(
                        ErrorCode::InvalidGeometry,
                        "the arc's end is its start",
                    ));
                }
                shape.start_parameter = from;
                shape.sweep = sweep;
            }
            return made(
                document,
                create_drawn(
                    "CREATE_ELLIPSE",
                    vec![drawn_entity(Geometry::Ellipse(shape), &attributes)],
                ),
            );
        }

        if verb == "XLINE" || verb == "RAY" {
            let ray = verb == "RAY";
            let parsed = if ray {
                split_arguments(args, 0, &[])?
            } else {
                split_arguments(args, 0, &["angle"])?
            };
            if parsed.positional.is_empty() {
                return Err(usage_error(if ray {
                    "RAY p p [p...]"
                } else {
                    "XLINE p p [p...] | XLINE p angle=dir"
                }));
            }
            let base = self.parse_drawing_point(document, &parsed.positional[0], None)?;
            let mut lines = Vec::new();
            for (_, value) in &parsed.options {
                let direction = parse_direction(value, document.drafting().angles)?;
                lines.push(construction_segment(
                    base.point,
                    Vec2::new(direction.cos(), direction.sin()),
                    ray,
                ));
            }
            for token in &parsed.positional[1..] {
                let through = self.parse_drawing_point(document, token, Some(base.point))?;
                let d = through.point - base.point;
                if d.length() <= tolerance::GEOMETRIC {
                    return Err(make_error(
                        ErrorCode::InvalidGeometry,
                        "a through point is the base point",
                        token.as_str(),
                    ));
                }
                lines.push(construction_segment(base.point, d.normalized(), ray));
            }
            if lines.is_empty() {
                return Err(usage_error(if ray {
                    "RAY p p [p...]"
                } else {
                    "XLINE p p [p...] | XLINE p angle=dir"
                }));
            }
            let command = create_construction(document.model(), attributes, &lines, ray);
            return made(document, command);
        }

        // DLINE
        let parsed = split_arguments(args, 0, &["width"])?;
        let Some(width) = parsed.find("width") else {
            return Err(usage_error("DLINE p p [p...] width= [CLOSE]"));
        };
        let w = positive_of(width, "width")?;
        let mut points = Vec::new();
        let mut closed = false;
        for (i, token) in parsed.positional.iter().enumerate() {
            let word = upper_of(token);
            if (word == "CLOSE" || word == "C") && i + 1 == parsed.positional.len() {
                closed = true;
                continue;
            }
            points.push(self.parse_drawing_point(document, token, None)?.point);
        }
        if points.len() < 2 || (closed && points.len() < 3) {
            return Err(usage_error("DLINE p p [p...] width= [CLOSE]"));
        }
        let sides = double_line_sides(&Polyline2::new(points, closed), w);
        if sides.len() != 2 {
            return Err(Error::new(
                ErrorCode::InvalidGeometry,
                "the path is too tight for a double line of this width",
            ));
        }
        let entities = sides
            .into_iter()
            .map(|side| drawn_entity(Geometry::Polyline(side), &attributes))
            .collect();
        made(document, create_drawn("CREATE_DOUBLE_LINE", entities))
    }
}

// ---- whole-polyline edits -----------------------------------------------------------------

fn polyline_verb(document: &mut Document, verb: &str, args: &[String]) -> Result<String> {
    if verb == "WEED" {
        let parsed = split_arguments(args, 0, &["tolerance", "keep"])?;
        let Some(tolerance_text) = parsed.find("tolerance") else {
            return Err(usage_error("WEED target tolerance= [keep=on|off]"));
        };
        let tol = positive_of(tolerance_text, "tolerance")?;
        let keep = match parsed.find("keep") {
            Some(text) => switch_of(text, "keep")?,
            None => true,
        };
        let ids = targets_of(document, &parsed.positional, false)?;
        let mut before: BTreeMap<EntityId, usize> = BTreeMap::new();
        for &id in &ids {
            before.insert(id, polyline_of(document, id)?.vertices.len());
        }
        document.execute(edit_each_polyline(
            ids.clone(),
            "WEED",
            move |model, _, p| {
                let survey_points = if keep {
                    vertices_on_survey_points(model, p, SURVEY_POINT_TOLERANCE)
                } else {
                    Vec::new()
                };
                weed(p, tol, &survey_points)
            },
        ))?;
        let records: Vec<String> = ids
            .iter()
            .map(|id| {
                format!(
                    "{} before={}",
                    polyline_record(document, *id),
                    before.get(id).copied().unwrap_or_default()
                )
            })
            .collect();
        return Ok(records.join("\n"));
    }

    if verb == "DENSIFY" {
        let parsed = split_arguments(args, 0, &["interval", "chord"])?;
        let interval = parsed.find("interval");
        let chord = parsed.find("chord");
        if interval.is_none() && chord.is_none() {
            return Err(usage_error("DENSIFY target interval= [chord=]"));
        }
        let step = match interval {
            Some(text) => number_of(text, "interval")?,
            None => 0.0,
        };
        let chord_tolerance = match chord {
            Some(text) => number_of(text, "chord")?,
            None => 0.0,
        };
        if step < 0.0 || chord_tolerance < 0.0 || (step == 0.0 && chord_tolerance == 0.0) {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "give an interval or a chord tolerance greater than zero",
            ));
        }
        let ids = targets_of(document, &parsed.positional, false)?;
        let command = edit_polylines(ids.clone(), "DENSIFY", move |p| {
            densify(p, step, chord_tolerance)
        });
        return run_records(document, &ids, command, "");
    }

    if verb == "CLOSE" || verb == "OPEN" {
        let ids = targets_of(document, args, true)?;
        let close = verb == "CLOSE";
        let command = edit_polylines(
            ids.clone(),
            if close { "CLOSE" } else { "OPEN" },
            move |p| {
                if p.closed == close {
                    return Ok(p.clone());
                }
                if close {
                    close_polyline(p)
                } else {
                    open_polyline(p)
                }
            },
        );
        return run_records(document, &ids, command, "");
    }

    // The verbs on one polyline, with vertex indices.
    if args.is_empty() {
        return Err(usage_error(match verb {
            "STRAIGHTEN" => "STRAIGHTEN id N N",
            "STARTVERTEX" => "STARTVERTEX id N",
            _ => "VERTEXZ id N z|none | target INTERPOLATE | id GRADE N N",
        }));
    }
    if verb == "VERTEXZ" && args.len() >= 2 && upper_of(&args[args.len() - 1]) == "INTERPOLATE" {
        let ids = targets_of(document, &args[..args.len() - 1], false)?;
        let command = edit_polylines(ids.clone(), "INTERPOLATE_Z", interpolate_heights);
        return run_records(document, &ids, command, "");
    }
    let id = entity_id_of(&args[0])?;
    let polyline = polyline_of(document, id)?;
    let one = [id];

    if verb == "STRAIGHTEN"
        || (verb == "VERTEXZ" && args.len() >= 4 && upper_of(&args[1]) == "GRADE")
    {
        let grade = verb == "VERTEXZ";
        let parsed = split_arguments(args, if grade { 2 } else { 1 }, &["side"])?;
        if parsed.positional.len() != 2 {
            return Err(usage_error(if grade {
                "VERTEXZ id GRADE N N [side=short|long]"
            } else {
                "STRAIGHTEN id N N [side=short|long]"
            }));
        }
        let from = index_in(&polyline, &parsed.positional[0], verb);
        let to = index_in(&polyline, &parsed.positional[1], verb);
        let a = from?;
        let b = to?;
        // Forward from the first number to the second - round through vertex
        // 0 of a closed polyline when the second is lower - as these verbs
        // have always walked, so a line written for them still takes out, or
        // regrades, the vertices it did. The window's Straighten and Grade
        // take a closed polyline's shorter side and their O the other: here
        // those are side=short and side=long, asked for (vertex_range, the
        // one rule). Made the default, the shorter side silently changed what
        // an existing script's or agent's line did to a closed polyline.
        let mut other_side: Option<bool> = None;
        if let Some(side) = parsed.find("side") {
            let word = lower_of(side);
            if word != "short" && word != "long" {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "side is short or long",
                    side.as_str(),
                ));
            }
            if word == "long" && !polyline.closed {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    format!(
                        "side=long goes the other way round a closed polyline, and polyline {id} \
                         is open"
                    ),
                ));
            }
            other_side = Some(word == "long");
        }
        let command = edit_polyline(
            id,
            if grade { "GRADE" } else { "STRAIGHTEN" },
            move |p: &CurvePolyline2| -> PolylineResult {
                let (walk_from, walk_to) = match other_side {
                    Some(other) => vertex_range(p, a, b, other),
                    None => (a, b),
                };
                if grade {
                    grade_between(p, walk_from, walk_to)
                } else {
                    straighten(p, walk_from, walk_to)
                }
            },
        );
        return run_records(document, &one, command, "");
    }
    if verb == "STARTVERTEX" {
        if args.len() != 2 {
            return Err(usage_error("STARTVERTEX id N"));
        }
        let at = index_in(&polyline, &args[1], verb)?;
        let command = edit_polyline(id, "START_VERTEX", move |p| change_start_vertex(p, at));
        return run_records(document, &one, command, "");
    }
    // VERTEXZ id N z|none
    if args.len() != 3 {
        return Err(usage_error(
            "VERTEXZ id N z|none | target INTERPOLATE | id GRADE N N",
        ));
    }
    let at = index_in(&polyline, &args[1], verb)?;
    let height = if lower_of(&args[2]) != "none" && args[2] != "-" {
        Some(number_of(&args[2], "the height")?)
    } else {
        None
    };
    let command = edit_polyline(id, "VERTEX_Z", move |p| set_vertex_height(p, at, height));
    run_records(document, &one, command, "")
}

// ---- drafting settings ----------------------------------------------------------------------

// ORTHO and the like: no word keeps the setting, on or off sets it.
fn flag(verb: &str, value: &mut bool, tokens: &[String]) -> Status {
    if tokens.len() > 1 {
        return Err(usage_error(&format!("{verb} [on|off]")));
    }
    if let Some(word) = tokens.first() {
        *value = switch_of(word, verb)?;
    }
    Ok(())
}

fn drafting_verb(document: &mut Document, verb: &str, args: &[String]) -> Result<String> {
    let mut settings = *document.drafting();
    match verb {
        "ORTHO" => flag(verb, &mut settings.ortho, args)?,
        "TRACKING" => flag(verb, &mut settings.object_tracking, args)?,
        "POLAR" => {
            let parsed = split_arguments(args, 0, &["increment"])?;
            flag(verb, &mut settings.polar, &parsed.positional)?;
            if let Some(increment) = parsed.find("increment") {
                let value = positive_of(increment, "increment")?;
                if value > 180.0 {
                    return Err(make_error(
                        ErrorCode::InvalidArgument,
                        "the polar increment is at most 180 degrees",
                        increment.as_str(),
                    ));
                }
                settings.polar_increment = value * DEG_TO_RAD;
            }
        }
        "ANGLES" => {
            if args.len() > 1 {
                return Err(usage_error("ANGLES [ccw|bearing]"));
            }
            if let Some(word) = args.first() {
                match lower_of(word).as_str() {
                    "bearing" | "bearings" => settings.angles = AngleConvention::Bearing,
                    "ccw" | "degrees" | "counterclockwise" => {
                        settings.angles = AngleConvention::Counterclockwise;
                    }
                    _ => {
                        return Err(make_error(
                            ErrorCode::ParseFailure,
                            "expected ccw or bearing",
                            word.as_str(),
                        ));
                    }
                }
            }
        }
        "LOCK" => {
            if args.len() == 1 && upper_of(&args[0]) == "OFF" {
                settings.angle_lock = None;
                settings.length_lock = None;
            } else {
                let parsed = split_arguments(args, 0, &["angle", "length"])?;
                if !parsed.positional.is_empty() {
                    return Err(usage_error(
                        "LOCK [angle=dir|none] [length=d|none] | LOCK OFF",
                    ));
                }
                if let Some(angle) = parsed.find("angle") {
                    settings.angle_lock = if lower_of(angle) == "none" {
                        None
                    } else {
                        Some(parse_direction(angle, settings.angles)?)
                    };
                }
                if let Some(length) = parsed.find("length") {
                    settings.length_lock = if lower_of(length) == "none" {
                        None
                    } else {
                        Some(positive_of(length, "length")?)
                    };
                }
            }
        }
        "SNAP" => {
            let parsed = split_arguments(args, 0, &["modes", "add", "remove"])?;
            flag(verb, &mut settings.snap_enabled, &parsed.positional)?;
            for (key, value) in &parsed.options {
                let modes = modes_of(value)?;
                match key.as_str() {
                    "modes" => settings.snap_modes = modes,
                    "add" => settings.snap_modes |= modes,
                    _ => settings.snap_modes &= !modes,
                }
            }
        }
        _ => {
            // DRAFTING
            if !args.is_empty() {
                return Err(usage_error("DRAFTING"));
            }
        }
    }
    if settings != *document.drafting() {
        *document.drafting_mut() = settings;
        document.notify_drafting_changed();
    }
    Ok(drafting_record(document.drafting()))
}

const DRAWING_HELP_TEXT: &str = r#"Drawing (docs/drawing.md): vertex indices count from 0; options are key=value;
          replies are key=value records.  target = id (12 or #12) | SELECTION.
          Points x,y[,z] | @dx,dy[,dz] | @dist<dir (dir in the ANGLES convention, or a
          quadrant bearing N45d30'E); a z is the vertex's height.  Each edit is one undo step.
Vertex    VERTEX LIST id   a summary record, then one per vertex: index x y z bulge
          bearing (whole-circle degrees) distance (the segment's chord)
          VERTEX INSERT id p|#id@x,y[,z]|#id.sN [after=N]   #id@x,y: on the line nearest x,y
          VERTEX DELETE id N [N...] | MOVE id N p (@ is from the vertex, dz changes its height)
          VERTEX SET id N [x= y= z=|none bulge= bearing= distance=]   as the Vertices panel
Polyline  WEED target tolerance= [keep=on|off]   (keep: survey-point vertices stay)
          DENSIFY target interval= [chord=]  | CLOSE target | OPEN [target]
          STRAIGHTEN id N N [side=short|long] | VERTEXZ id GRADE N N [side=short|long]
          (forward from the first N; on a closed polyline side=short takes the side
          with fewer vertices between, as the window's tools do, and side=long the other)
          STARTVERTEX id N | VERTEXZ id N z|none | VERTEXZ target INTERPOLATE
          (OPEN takes #ids or SELECTION; OPEN directory opens a project)
Draw      PLINE p p [ARC p...] [LINE p...] [CLOSE]   ARC: tangent arcs; heights as x,y,z
          PLINE3D p p [p...] [CLOSE] [z=]   z= is the height of points given without one
          SPLINE p p [p...] [control=on|off] [degree=3] | DLINE p p [p...] width= [CLOSE]
          ELLIPSE centre axis-end minor=|ratio= [start=deg end=deg]   (eccentric anomaly)
          XLINE p p [p...] | XLINE p angle=dir [angle=...] | RAY p p [p...]   construction
          layer: drawn and snapped to, never plotted
Drafting  ORTHO [on|off] | POLAR [on|off] [increment=deg] | TRACKING [on|off]
          ANGLES [ccw|bearing] | LOCK [angle=dir|none] [length=d|none] | LOCK OFF
          SNAP [on|off] [modes=a,b|all|none] [add=a,b] [remove=a,b] | DRAFTING
          each replies with the whole drafting record; not undo steps
"#;
