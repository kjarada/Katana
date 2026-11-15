//! The Vertices panel's table, headless (docs/drawing.md, "The Vertices
//! panel"; include/katana/cad/drawing/vertex_table.hpp,
//! src/katana_cad/drawing/vertex_table.cpp): one row per vertex of a
//! polyline, with its index, easting, northing, height and bulge, and the
//! bearing and distance of the segment that starts there; and what typing
//! into a cell does. The panel only shows these rows and hands edits back; the VERTEX LIST
//! and VERTEX SET verbs read and write the same columns, so a value typed in
//! the panel and one sent by an agent are the same edit.

use katana_core::text::{equals_ignoring_case, parse_finite_double};
use katana_core::{Error, ErrorCode};
use katana_entity::EnumName;
use katana_geometry::polyline_vertices::{move_vertex, set_segment_bulge, set_vertex_height};
use katana_geometry::{CurvePolyline2, PolylineResult, Vec2};

use crate::drawing::drafting::{AngleConvention, format_bearing, parse_direction};
use crate::survey_tools::c_fixed;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum VertexColumn {
    #[default]
    Index,
    Easting,
    Northing,
    Height,
    Bulge,
    /// Of the segment that starts at the vertex.
    Bearing,
    /// Its chord's length.
    Distance,
}

pub const VERTEX_COLUMN_COUNT: i32 = 7;

impl EnumName for VertexColumn {
    fn name(self) -> &'static str {
        match self {
            VertexColumn::Index => "Index",
            VertexColumn::Easting => "Easting",
            VertexColumn::Northing => "Northing",
            VertexColumn::Height => "Height",
            VertexColumn::Bulge => "Bulge",
            VertexColumn::Bearing => "Bearing",
            VertexColumn::Distance => "Distance",
        }
    }
}

/// "e", "easting", "x" -> Easting and so on, case-insensitively: the names
/// the VERTEX SET verb takes. None for an unknown name.
pub fn vertex_column_from_string(name: &str) -> Option<VertexColumn> {
    let lowered = trimmed(name).to_ascii_lowercase();
    match lowered.as_str() {
        "e" | "x" | "easting" => Some(VertexColumn::Easting),
        "n" | "y" | "northing" => Some(VertexColumn::Northing),
        "z" | "h" | "height" | "elevation" => Some(VertexColumn::Height),
        "bulge" | "b" => Some(VertexColumn::Bulge),
        "bearing" | "brg" => Some(VertexColumn::Bearing),
        "distance" | "dist" | "d" => Some(VertexColumn::Distance),
        _ => None,
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct VertexRow {
    pub index: usize,
    pub easting: f64,
    pub northing: f64,
    pub height: Option<f64>,
    pub bulge: f64,
    /// The segment starting here; None at the last vertex of an open
    /// polyline, which starts none. Radians, counter-clockwise from east (of
    /// the chord).
    pub bearing: Option<f64>,
    /// The chord's length.
    pub distance: Option<f64>,
}

/// core's trimmed over text: it drops ASCII bytes only, so the rest is UTF-8.
fn trimmed(text: &str) -> &str {
    std::str::from_utf8(katana_core::text::trimmed(text.as_bytes())).unwrap_or_default()
}

pub fn vertex_rows(polyline: &CurvePolyline2) -> Vec<VertexRow> {
    let mut rows = Vec::with_capacity(polyline.vertices.len());
    for (index, vertex) in polyline.vertices.iter().enumerate() {
        let mut row = VertexRow {
            index,
            easting: vertex.position.x,
            northing: vertex.position.y,
            height: vertex.height,
            bulge: vertex.bulge,
            bearing: None,
            distance: None,
        };
        if index < polyline.segment_count() {
            let next = polyline.vertices[polyline.segment_end(index)].position;
            // The chord's bearing and length, as a traverse table gives an
            // arc: what editing either cell changes (edit_vertex_cell).
            row.bearing = Some((next - vertex.position).angle());
            row.distance = Some((next - vertex.position).length());
        }
        rows.push(row);
    }
    rows
}

/// A cell as the panel shows it: coordinates to 3 places, a bearing as a
/// whole-circle D°MM'SS", no height as empty.
pub fn cell_text(row: &VertexRow, column: VertexColumn) -> String {
    // The C++'s snprintf "%.3f" and "%.6f".
    match column {
        VertexColumn::Index => format!("{}", row.index),
        VertexColumn::Easting => c_fixed(row.easting, 3),
        VertexColumn::Northing => c_fixed(row.northing, 3),
        VertexColumn::Height => row
            .height
            .map_or_else(String::new, |height| c_fixed(height, 3)),
        VertexColumn::Bulge => {
            if row.bulge == 0.0 {
                String::from("0")
            } else {
                c_fixed(row.bulge, 6)
            }
        }
        VertexColumn::Bearing => row
            .bearing
            .map_or_else(String::new, |bearing| format_bearing(bearing, 0)),
        VertexColumn::Distance => row
            .distance
            .map_or_else(String::new, |distance| c_fixed(distance, 3)),
    }
}

fn refused(why: String) -> Error {
    Error::new(ErrorCode::InvalidArgument, why)
}

/// The polyline after `text` is typed into row `index`, column `column`:
///
/// ```text
///   Easting / Northing   the vertex moves
///   Height               the vertex's height; empty or "none" clears it
///   Bulge                the segment's bulge (0 straight)
///   Bearing              the NEXT vertex moves to keep the segment's length
///                        on the typed bearing (angle convention as given)
///   Distance             the next vertex moves along the chord's bearing to
///                        that chord length (an arc keeps its bulge, so its
///                        shape; the arc length follows)
/// ```
///
/// InvalidArgument for a column that cannot be edited (Index), a value that
/// does not parse, or a bearing or distance at the last vertex of an open
/// polyline. The C++ defaults `angles` to AngleConvention::Bearing.
pub fn edit_vertex_cell(
    polyline: &CurvePolyline2,
    index: usize,
    column: VertexColumn,
    text: &str,
    angles: AngleConvention,
) -> PolylineResult {
    if index >= polyline.vertices.len() {
        return Err(refused(format!("there is no vertex {index}")));
    }
    let value = trimmed(text);
    let number = || parse_finite_double(value);
    match column {
        VertexColumn::Index => Err(refused(String::from(
            "a vertex's index is its place in the polyline and cannot be typed",
        ))),
        VertexColumn::Easting | VertexColumn::Northing => {
            let Some(coordinate) = number() else {
                return Err(refused(format!("'{value}' is not a coordinate")));
            };
            let mut moved = polyline.vertices[index].position;
            if column == VertexColumn::Easting {
                moved.x = coordinate;
            } else {
                moved.y = coordinate;
            }
            move_vertex(polyline, index, moved)
        }
        VertexColumn::Height => {
            if value.is_empty() || equals_ignoring_case(value.as_bytes(), b"none") {
                return set_vertex_height(polyline, index, None);
            }
            let Some(height) = number() else {
                return Err(refused(format!(
                    "'{value}' is not a height; type a number or none"
                )));
            };
            set_vertex_height(polyline, index, Some(height))
        }
        VertexColumn::Bulge => {
            let Some(bulge) = number() else {
                return Err(refused(format!("'{value}' is not a bulge")));
            };
            set_segment_bulge(polyline, index, bulge)
        }
        VertexColumn::Bearing | VertexColumn::Distance => {
            if index >= polyline.segment_count() {
                return Err(refused(String::from(
                    "the last vertex of an open polyline starts no segment",
                )));
            }
            let next = polyline.segment_end(index);
            let from = polyline.vertices[index].position;
            let chord = polyline.vertices[next].position - from;
            let mut direction = chord.angle();
            let mut length = chord.length();
            if column == VertexColumn::Bearing {
                direction = parse_direction(value, angles)?;
            } else {
                match number() {
                    Some(parsed) if parsed > 0.0 => length = parsed,
                    _ => {
                        return Err(refused(format!(
                            "'{value}' is not a distance greater than zero"
                        )));
                    }
                }
            }
            move_vertex(
                polyline,
                next,
                from + Vec2::new(direction.cos(), direction.sin()) * length,
            )
        }
    }
}
