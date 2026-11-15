//! What a surface built from the drawing is built from
//! (include/katana/cad/geo/surface_input.hpp; docs/terrain.md, "Surfaces on
//! every front end"): the points and breaklines of the entities a scope took,
//! at the heights their `elevation` / `elevations` properties give them
//! (entity's heights_of, the one reader).
//!
//! Here, in the cad layer, rather than in the window, because katana_cli,
//! katana_mcp and the window all build a surface from the drawing through the
//! SURFACE FROM verb, and the rules must have one definition. They are the
//! ones the window's Surface From Drawing kept, with one change:
//!
//! * A point is a survey point; a polyline, and a line, a breakline through
//!   its vertices, closed when the polyline is. A curve polyline (one with
//!   arcs, holding its heights itself) is a breakline through its chords at
//!   geometry's CURVE_CHORD_TOLERANCE, each chord point at the height
//!   interpolated by length along its segment - none where an end has none,
//!   which then breaks the breakline as a missing height does.
//! * A vertex with no height is left out and counted, and a breakline is
//!   broken there: a null is "not surveyed", and triangulating it at zero
//!   would dig a pit to the datum under it.
//! * A ring closes only when it lost none of its vertices.
//! * The change: an entity with no height property at all is left out and
//!   counted too, as a vertex with a null height is. The window used to put
//!   it on the datum (z = 0) - a whole 2D drawing then became a flat surface
//!   at 0, and one plain line among levelled strings a trench to the datum
//!   (absent is not zero, CLAUDE.md section 11).
//!
//! Everything else a scope takes (text, arcs, circles, ellipses, splines,
//! dimensions ...) is skipped and counted by its type, never silently
//! dropped.

use std::collections::BTreeMap;

use katana_entity::entity::heights_of;
use katana_entity::{EntityId, Geometry, Model};
use katana_geometry::curves2d::CURVE_CHORD_TOLERANCE;
use katana_geometry::{Point2, Point3};
use katana_terrain::{Breakline, TinInput};

use crate::geo::drape::type_word;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SurfaceInput {
    pub input: TinInput,
    /// Entities that gave at least one point.
    pub used: usize,
    /// Vertices left out for having no height, over every entity.
    pub heightless_vertices: usize,
    /// Entities left out, by reason: "heightless" (a point or line of which
    /// no vertex has a height), or the type of an entity no surface is built
    /// from ("text", "arc", "circle" ...).
    pub skipped: BTreeMap<String, usize>,
}

fn flush(breakline: &mut Breakline, out: &mut SurfaceInput) {
    if breakline.vertices.len() >= 2 {
        out.input.breaklines.push(breakline.clone());
    }
    breakline.vertices.clear();
}

// The vertices of one run of line: a polyline's, a line's two ends, a curve
// polyline's chord points.
fn add_line(
    vertices: &[Point2],
    closed: bool,
    heights: &[Option<f64>],
    out: &mut SurfaceInput,
    gave_points: &mut bool,
) {
    let mut breakline = Breakline::default();
    let mut whole = true;
    for (vertex, &z) in vertices.iter().zip(heights) {
        let Some(z) = z else {
            out.heightless_vertices += 1;
            whole = false;
            flush(&mut breakline, out);
            continue;
        };
        let point = Point3::new(vertex.x, vertex.y, z);
        breakline.vertices.push(point);
        out.input.points.push(point);
        *gave_points = true;
    }
    // Closing only makes sense for a ring that lost none of its vertices.
    breakline.closed = closed && whole;
    flush(&mut breakline, out);
}

/// The survey points and breaklines of `ids`, in their order; an id the model
/// has not is passed over. Deterministic: the same ids give the same input.
pub fn surface_input(model: &Model, ids: &[EntityId]) -> SurfaceInput {
    let mut out = SurfaceInput::default();
    for &id in ids {
        let Some(entity) = model.entities.find(id) else {
            continue;
        };
        let mut gave_points = false;
        match &entity.geometry {
            Geometry::Point(point) => {
                if let Some(z) = heights_of(&entity.properties, 1)[0] {
                    out.input
                        .points
                        .push(Point3::new(point.position.x, point.position.y, z));
                    gave_points = true;
                } else {
                    out.heightless_vertices += 1;
                }
            }
            Geometry::Polyline(polyline) => add_line(
                &polyline.vertices,
                polyline.closed,
                &heights_of(&entity.properties, polyline.vertices.len()),
                &mut out,
                &mut gave_points,
            ),
            Geometry::CurvePolyline(curve) => {
                // A string with arcs holds its heights itself; its arcs go in
                // as chords at the drawing's tolerance, each chord point at
                // the height interpolated by length along its segment and none
                // where an end has none (CurvePolyline2::height_at_station) -
                // the rule the window's Surface From Drawing had for it.
                let mut walk = curve.tessellate_with_heights(CURVE_CHORD_TOLERANCE);
                if curve.closed && walk.len() > 1 {
                    walk.pop(); // the closing point repeats the first
                }
                let positions: Vec<Point2> = walk.iter().map(|step| step.position).collect();
                let heights: Vec<Option<f64>> = walk.iter().map(|step| step.height).collect();
                add_line(
                    &positions,
                    curve.closed,
                    &heights,
                    &mut out,
                    &mut gave_points,
                );
            }
            Geometry::Line(line) => add_line(
                &[line.start, line.end],
                false,
                &heights_of(&entity.properties, 2),
                &mut out,
                &mut gave_points,
            ),
            _ => {
                *out.skipped.entry(type_word(entity)).or_default() += 1;
                continue;
            }
        }
        if gave_points {
            out.used += 1;
        } else {
            *out.skipped.entry(String::from("heightless")).or_default() += 1;
        }
    }
    out
}
