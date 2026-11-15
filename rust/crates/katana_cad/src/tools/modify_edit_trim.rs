//! Trim and Extend (src/katana_cad/tools/modify_edit_trim.cpp): choose the
//! edges, then pick the objects, as many as wanted, Enter to finish. They
//! are one tool with two modes because everything but the geometry is the
//! same - the edges step, "every object" when Enter is pressed at once, the
//! repeated picks and their Undo.

use std::cmp::Ordering;

use katana_core::{Error, ErrorCode, Result};
use katana_entity::entity_geometry::{bounding_box, distance_to};
use katana_entity::{Entity, EntityId, EntityType, Geometry};
use katana_geometry::editing::{bounding_box as curve_bounding_box, extend, trim};
use katana_geometry::intersection::intersect;
use katana_geometry::polyline_vertices::{sub_path, wrapping_path};
use katana_geometry::{
    Arc2, Curve2, CurvePolyline2, CurveSegment, IntersectionKind, Point2, Polyline2, Segment2,
};
use katana_math::cpp;
use katana_math::numerics::{TWO_PI, tolerance};

use crate::document::Document;
use crate::drawing::vertex_editing::write_polyline;
use crate::interactive_tool::{InteractiveTool, ToolContext, ToolInput, ToolStart, ToolStep};
use crate::layer_overrides::NO_LAYER_OVERRIDES;
use crate::selection::{SelectionFilter, is_drawn, pick_entity};
use crate::tool_feedback::ToolFeedback;
use crate::tools::families::keep_work_on_escape;
use crate::tools::modify_edit_support::{
    EditSession, PolylinePath, ToolPtr, append_edges, as_curve, as_geometry, as_sentence,
    carry_heights, is_option, kind_name, nearest_piece, no_drawing, refusal_to_edit, selection_now,
};

// ---- trimming one piece ------------------------------------------------------------

#[derive(Default)]
struct Cut {
    remaining: Vec<Geometry>,
    /// For the preview: what the pick takes away.
    removed: Vec<Geometry>,
}

fn by_value<T: PartialOrd>(a: &T, b: &T) -> Ordering {
    a.partial_cmp(b).unwrap_or(Ordering::Equal)
}

// The gaps a trim left in [0, 1] once `kept` (fractions along the original)
// are taken out.
fn gaps(mut kept: Vec<(f64, f64)>, tiny: f64) -> Vec<(f64, f64)> {
    kept.sort_by(by_value);
    let mut out = Vec::new();
    let mut cursor: f64 = 0.0;
    for (from, to) in kept {
        if from - cursor > tiny {
            out.push((cursor, from));
        }
        cursor = cpp::max(cursor, to);
    }
    if 1.0 - cursor > tiny {
        out.push((cursor, 1.0));
    }
    out
}

fn removed_parts(original: &Curve2, remaining: &[Curve2]) -> Vec<Geometry> {
    let mut out = Vec::new();
    match original {
        Curve2::Segment(segment) => {
            let mut kept = Vec::new();
            for piece in remaining {
                if let Curve2::Segment(part) = piece {
                    kept.push((
                        segment.parameter_of(part.start),
                        segment.parameter_of(part.end),
                    ));
                }
            }
            for (from, to) in gaps(kept, tolerance::GEOMETRIC / segment.length()) {
                out.push(Geometry::Line(Segment2::new(
                    segment.point_at(from),
                    segment.point_at(to),
                )));
            }
        }
        Curve2::Arc(arc) => {
            let mut kept = Vec::new();
            for piece in remaining {
                if let Curve2::Arc(part) = piece {
                    let from = (part.start_angle - arc.start_angle) / arc.sweep;
                    kept.push((from, from + part.sweep / arc.sweep));
                }
            }
            for (from, to) in gaps(kept, tolerance::GEOMETRIC / arc.length()) {
                out.push(Geometry::Arc(Arc2::new(
                    arc.center,
                    arc.radius,
                    arc.start_angle + arc.sweep * from,
                    arc.sweep * (to - from),
                )));
            }
        }
        Curve2::Circle(circle) => match remaining {
            // A trimmed circle is the one arc it keeps; the rest of the turn
            // went.
            [Curve2::Arc(kept)] => out.push(Geometry::Arc(Arc2::new(
                circle.center,
                circle.radius,
                kept.start_angle + kept.sweep,
                TWO_PI - kept.sweep,
            ))),
            _ => out.push(Geometry::Circle(*circle)),
        },
    }
    out
}

fn sort_unique(values: &mut Vec<f64>) {
    values.sort_by(by_value);
    values.dedup_by(|current, kept| *current - *kept <= tolerance::GEOMETRIC);
}

// Whether a cut lies on an end of an open path, where it cuts nothing off.
fn at_an_end(station: f64, length: f64) -> bool {
    station <= tolerance::GEOMETRIC || station >= length - tolerance::GEOMETRIC
}

// The span of `cuts` (ascending) either side of the station `at` of an open
// path `length` long.
fn span_about(cuts: &[f64], at: f64, length: f64) -> (f64, f64) {
    let mut low = 0.0;
    let mut high = length;
    for &cut in cuts {
        if cut <= at {
            low = cut;
        } else {
            high = cut;
            break;
        }
    }
    (low, high)
}

// geometry's trim for a polyline, which it does not take: the crossings are
// measured along the polyline and the span between the two either side of
// the pick goes, exactly as trim() does for a line.
fn trim_polyline(polyline: &Polyline2, cutters: &[Curve2], pick: Point2) -> Result<Cut> {
    let path = PolylinePath::new(polyline.clone());
    let mut cuts = Vec::new();
    for i in 0..path.segment_count() {
        let segment = path.segment(i);
        if segment.is_degenerate() {
            continue;
        }
        for cutter in cutters {
            let hit = intersect(&Curve2::Segment(segment), cutter);
            if hit.kind != IntersectionKind::Points {
                continue;
            }
            for &point in &hit.points[..hit.count] {
                cuts.push(path.station_on(i, point));
            }
        }
    }
    let length = path.length();
    let at = path.station_of(pick);
    let mut out = Cut::default();
    if !path.closed() {
        cuts.retain(|&station| {
            let erase = at_an_end(station, length);
            !erase
        });
        sort_unique(&mut cuts);
        if cuts.is_empty() {
            return Err(Error::new(
                ErrorCode::InvalidGeometry,
                "no cutting edge crosses that polyline",
            ));
        }
        let (low, high) = span_about(&cuts, at, length);
        if low > 0.0 {
            out.remaining
                .push(Geometry::Polyline(path.between(0.0, low)));
        }
        if high < length {
            out.remaining
                .push(Geometry::Polyline(path.between(high, length)));
        }
        out.removed
            .push(Geometry::Polyline(path.between(low, high)));
        return Ok(out);
    }
    // Closed: a crossing at the end of the closing segment is the one at the
    // start, seen from the other side of the seam.
    for cut in &mut cuts {
        if *cut >= length - tolerance::GEOMETRIC {
            *cut = 0.0;
        }
    }
    sort_unique(&mut cuts);
    if cuts.len() < 2 {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            "a closed polyline needs two crossings with the cutting edges to be trimmed",
        ));
    }
    // std::upper_bound: the first cut `at` is less than.
    let above = cuts.partition_point(|&cut| !(at < cut));
    let first = cuts[0];
    let last = cuts[cuts.len() - 1];
    if above == 0 || above == cuts.len() {
        // The pick is in the span that runs through the first vertex.
        out.remaining
            .push(Geometry::Polyline(path.between(first, last)));
        out.removed
            .push(Geometry::Polyline(path.wrapping(last, first)));
    } else {
        let low = cuts[above - 1];
        let high = cuts[above];
        out.remaining
            .push(Geometry::Polyline(path.wrapping(high, low)));
        out.removed
            .push(Geometry::Polyline(path.between(low, high)));
    }
    Ok(out)
}

// The same for a curve polyline (docs/drawing.md): the crossings with its
// lines and arcs measured along it, and the pieces cut by geometry's
// sub_path, so an arc cut part-way keeps its circle.
fn trim_curve_polyline(polyline: &CurvePolyline2, cutters: &[Curve2], pick: Point2) -> Result<Cut> {
    let mut cuts = Vec::new();
    for i in 0..polyline.segment_count() {
        let length = polyline.segment_length(i);
        if !(length > tolerance::GEOMETRIC) {
            continue;
        }
        let start = polyline.station_of_vertex(i);
        let piece = polyline.segment(i);
        let curve = match piece {
            CurveSegment::Segment(segment) => Curve2::Segment(segment),
            CurveSegment::Arc(arc) => Curve2::Arc(arc),
        };
        for cutter in cutters {
            let hit = intersect(&curve, cutter);
            if hit.kind != IntersectionKind::Points {
                continue;
            }
            for &p in &hit.points[..hit.count] {
                let t = match piece {
                    CurveSegment::Segment(segment) => segment.parameter_of(p),
                    CurveSegment::Arc(arc) => arc.parameter_of_angle((p - arc.center).angle()),
                };
                cuts.push(start + cpp::clamp(t, 0.0, 1.0) * length);
            }
        }
    }
    let length = polyline.length();
    let at = polyline
        .nearest(pick)
        .map_or(0.0, |nearest| nearest.station);
    let mut out = Cut::default();
    if !polyline.closed {
        cuts.retain(|&station| {
            let erase = at_an_end(station, length);
            !erase
        });
        sort_unique(&mut cuts);
        if cuts.is_empty() {
            return Err(Error::new(
                ErrorCode::InvalidGeometry,
                "no cutting edge crosses that polyline",
            ));
        }
        let (low, high) = span_about(&cuts, at, length);
        if low > 0.0 {
            out.remaining
                .push(Geometry::CurvePolyline(sub_path(polyline, 0.0, low)));
        }
        if high < length {
            out.remaining
                .push(Geometry::CurvePolyline(sub_path(polyline, high, length)));
        }
        out.removed
            .push(Geometry::CurvePolyline(sub_path(polyline, low, high)));
        return Ok(out);
    }
    for cut in &mut cuts {
        if *cut >= length - tolerance::GEOMETRIC {
            *cut = 0.0;
        }
    }
    sort_unique(&mut cuts);
    if cuts.len() < 2 {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            "a closed polyline needs two crossings with the cutting edges to be trimmed",
        ));
    }
    let above = cuts.partition_point(|&cut| !(at < cut));
    let first = cuts[0];
    let last = cuts[cuts.len() - 1];
    if above == 0 || above == cuts.len() {
        out.remaining
            .push(Geometry::CurvePolyline(sub_path(polyline, first, last)));
        out.removed.push(Geometry::CurvePolyline(wrapping_path(
            polyline, last, first,
        )));
    } else {
        let low = cuts[above - 1];
        let high = cuts[above];
        out.remaining
            .push(Geometry::CurvePolyline(wrapping_path(polyline, high, low)));
        out.removed
            .push(Geometry::CurvePolyline(sub_path(polyline, low, high)));
    }
    Ok(out)
}

fn trim_geometry(piece: &Geometry, cutters: &[Curve2], pick: Point2) -> Result<Cut> {
    match piece {
        Geometry::Polyline(polyline) => trim_polyline(polyline, cutters, pick),
        Geometry::CurvePolyline(curved) => trim_curve_polyline(curved, cutters, pick),
        _ => {
            let Some(curve) = as_curve(piece) else {
                return Err(Error::new(
                    ErrorCode::Unsupported,
                    "only lines, arcs, circles and polylines trim",
                ));
            };
            let trimmed = trim(&curve, cutters, pick)?;
            Ok(Cut {
                remaining: trimmed.remaining.iter().map(as_geometry).collect(),
                removed: removed_parts(&curve, &trimmed.remaining),
            })
        }
    }
}

// ---- extending one piece -----------------------------------------------------------

struct Grow {
    geometry: Geometry,
    /// For the preview: the new length.
    added: Geometry,
}

fn extend_geometry(piece: &Geometry, boundaries: &[Curve2], pick: Point2) -> Result<Grow> {
    match piece {
        Geometry::Line(segment) => {
            let Curve2::Segment(grown) = extend(&Curve2::Segment(*segment), boundaries, pick)?
            else {
                unreachable!("extending a line makes a line");
            };
            let end_moved = grown.start == segment.start;
            Ok(Grow {
                geometry: Geometry::Line(grown),
                added: Geometry::Line(if end_moved {
                    Segment2::new(segment.end, grown.end)
                } else {
                    Segment2::new(grown.start, segment.start)
                }),
            })
        }
        Geometry::Arc(arc) => {
            let Curve2::Arc(grown) = extend(&Curve2::Arc(*arc), boundaries, pick)? else {
                unreachable!("extending an arc makes an arc");
            };
            let extra = grown.sweep - arc.sweep;
            let end_moved = (grown.start_angle - arc.start_angle).abs() <= tolerance::ANGULAR;
            Ok(Grow {
                geometry: Geometry::Arc(grown),
                added: Geometry::Arc(Arc2::new(
                    arc.center,
                    arc.radius,
                    if end_moved {
                        arc.end_angle()
                    } else {
                        grown.start_angle
                    },
                    extra,
                )),
            })
        }
        Geometry::Polyline(polyline) => {
            // The end nearer the pick along the polyline, whatever vertex the
            // pick happens to be closest to: an interior vertex is not an end.
            let path = PolylinePath::new(polyline.clone());
            let at_start = path.station_of(pick) < 0.5 * path.length();
            let v = &polyline.vertices;
            let n = v.len();
            let end = if at_start {
                Segment2::new(v[1], v[0])
            } else {
                Segment2::new(v[n - 2], v[n - 1])
            };
            if end.is_degenerate() {
                return Err(Error::new(
                    ErrorCode::Unsupported,
                    "the end segment of that polyline has no length",
                ));
            }
            let Curve2::Segment(extended) = extend(&Curve2::Segment(end), boundaries, end.end)?
            else {
                unreachable!("extending a line makes a line");
            };
            let reached = extended.end;
            let mut grown = polyline.clone();
            grown.vertices[if at_start { 0 } else { n - 1 }] = reached;
            Ok(Grow {
                geometry: Geometry::Polyline(grown),
                added: Geometry::Line(Segment2::new(end.end, reached)),
            })
        }
        _ => Err(Error::new(
            ErrorCode::Unsupported,
            "only lines, arcs and open polylines extend",
        )),
    }
}

// ---- the tool ----------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Trim,
    Extend,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Edges,
    Targets,
}

struct Outcome {
    /// The entity's pieces after the operation.
    pieces: Vec<Entity>,
    /// What the preview shows of it.
    mark: Vec<Geometry>,
}

struct EdgeTool {
    mode: Mode,
    /// The selection the tool started with (ToolStart).
    selection: Vec<EntityId>,
    step: Step,
    /// Edges picked in the Edges step.
    picked: Vec<EntityId>,
    /// The edges chosen; empty when `all`.
    edges: Vec<EntityId>,
    all: bool,
    /// When `all`.
    every_edge: Vec<(EntityId, Curve2)>,
    session: EditSession,
    /// Per operation, for the preview.
    marks: Vec<Vec<Geometry>>,
}

impl EdgeTool {
    fn new(start: &ToolStart, mode: Mode) -> Self {
        Self {
            mode,
            selection: start.selection.clone(),
            step: Step::Edges,
            picked: Vec::new(),
            edges: Vec::new(),
            all: false,
            every_edge: Vec::new(),
            session: EditSession::new(),
            marks: Vec::new(),
        }
    }

    fn pick_target(&mut self, cx: &ToolContext<'_>, id: EntityId, at: Point2) -> ToolStep {
        let Some(document) = cx.document else {
            return no_drawing();
        };
        if let Some(refusal) = refusal_to_edit(document, id) {
            return ToolStep::rejected(refusal);
        }
        let outcome = match self.attempt(cx, document, id, at) {
            Ok(outcome) => outcome,
            Err(error) => return ToolStep::rejected(error.message),
        };
        self.session.begin();
        if let Err(error) = self.session.replace(id, outcome.pieces) {
            return ToolStep::rejected(error.message);
        }
        self.marks.push(outcome.mark);
        ToolStep::next("")
    }

    // The operation a pick at `at` on `id` would do, without doing it. The
    // error's message is the sentence to refuse the pick with.
    fn attempt(
        &self,
        cx: &ToolContext<'_>,
        document: &Document,
        id: EntityId,
        at: Point2,
    ) -> Result<Outcome> {
        let trimming = self.mode == Mode::Trim;
        let original = self.session.original(document, id);
        let pieces = self.session.pieces(document, id);
        let (Some(original), Some(index)) = (original, nearest_piece(&pieces, at)) else {
            return Err(Error::new(
                ErrorCode::NotFound,
                "That object has already been trimmed away.",
            ));
        };
        let piece = &pieces[index];
        let kind = kind_name(&piece.geometry);
        // A pick on a part an earlier pick removed: the document still draws
        // it, so the view picked the entity, but nothing of it is left there.
        let piece_distance = distance_to(&piece.geometry, at);
        let original_distance = distance_to(&original.geometry, at);
        if piece_distance > cpp::max(cx.pick_tolerance, original_distance) + tolerance::GEOMETRIC {
            return Err(Error::new(
                ErrorCode::NotFound,
                format!("That part of the {kind} has already been trimmed away."),
            ));
        }
        let entity_type = piece.r#type();
        let closed = match &piece.geometry {
            Geometry::Polyline(polyline) => polyline.closed,
            Geometry::CurvePolyline(curved) => curved.closed,
            _ => false,
        };
        if trimming
            && entity_type != EntityType::Line
            && entity_type != EntityType::Arc
            && entity_type != EntityType::Circle
            && entity_type != EntityType::Polyline
            && entity_type != EntityType::CurvePolyline
        {
            return Err(Error::new(
                ErrorCode::Unsupported,
                format!("A {kind} cannot be trimmed; pick a line, arc, circle or polyline."),
            ));
        }
        if !trimming {
            if entity_type == EntityType::Circle {
                return Err(Error::new(
                    ErrorCode::Unsupported,
                    "A circle has no end to extend.",
                ));
            }
            if closed {
                return Err(Error::new(
                    ErrorCode::Unsupported,
                    "A closed polyline has no end to extend.",
                ));
            }
            if entity_type != EntityType::Line
                && entity_type != EntityType::Arc
                && entity_type != EntityType::Polyline
            {
                return Err(Error::new(
                    ErrorCode::Unsupported,
                    format!("A {kind} cannot be extended; pick a line, arc or polyline."),
                ));
            }
        }

        let edges = self.edges_for(document, id, &piece.geometry);
        let mut mark = Vec::new();
        let mut replacement: Vec<Entity> = pieces[..index].to_vec();
        if trimming {
            let Ok(cut) = trim_geometry(&piece.geometry, &edges, at) else {
                return Err(Error::new(
                    ErrorCode::InvalidGeometry,
                    if entity_type == EntityType::Circle {
                        String::from(
                            "A circle needs two crossings with the cutting edges to be trimmed.",
                        )
                    } else if closed {
                        String::from(
                            "A closed polyline needs two crossings with the cutting edges to be \
                             trimmed.",
                        )
                    } else {
                        format!("No cutting edge crosses that {kind}.")
                    },
                ));
            };
            for remaining in cut.remaining {
                if let Geometry::CurvePolyline(kept) = &remaining {
                    // Its heights are in the geometry; stored as the
                    // simplest kind.
                    replacement.push(write_polyline(piece.clone(), kept)?);
                    continue;
                }
                let mut part = piece.clone();
                part.geometry = remaining;
                carry_heights(original, &mut part);
                replacement.push(part);
            }
            mark = cut.removed;
        } else {
            let grown = match extend_geometry(&piece.geometry, &edges, at) {
                Ok(grown) => grown,
                // Unsupported is extend_geometry's own refusal, already a
                // reason; anything else is the geometry finding nothing
                // ahead.
                Err(error) => {
                    return Err(Error::new(
                        ErrorCode::InvalidGeometry,
                        if error.code == ErrorCode::Unsupported {
                            as_sentence(&error.message)
                        } else {
                            format!("No boundary lies ahead of that end of the {kind}.")
                        },
                    ));
                }
            };
            let mut part = piece.clone();
            part.geometry = grown.geometry;
            carry_heights(original, &mut part);
            replacement.push(part);
            mark.push(grown.added);
        }
        replacement.extend_from_slice(&pieces[index + 1..]);
        Ok(Outcome {
            pieces: replacement,
            mark,
        })
    }

    // The curves that cut or bound `target`: the chosen edges as the session
    // has made them, never the target itself.
    fn edges_for(&self, document: &Document, target: EntityId, piece: &Geometry) -> Vec<Curve2> {
        let mut curves = Vec::new();
        let from_session = |id: EntityId, curves: &mut Vec<Curve2>| {
            if id == target {
                return;
            }
            for part in self.session.pieces(document, id) {
                append_edges(&part.geometry, curves);
            }
        };
        if !self.all {
            for &id in &self.edges {
                from_session(id, &mut curves);
            }
            return curves;
        }
        // Every object: the cache of the drawing's edges, except where the
        // session has changed an entity, which it answers for instead. A
        // trim cuts only where edges cross the piece, so edges nowhere near
        // it are passed over; an extend reaches out along the piece, so it
        // takes all.
        let near = bounding_box(piece).inflated(tolerance::GEOMETRIC);
        for &(id, curve) in &self.every_edge {
            if id == target || self.session.edited(id) {
                continue;
            }
            if self.mode == Mode::Trim && !curve_bounding_box(&curve).intersects(near) {
                continue;
            }
            curves.push(curve);
        }
        for id in self.session.edited_ids() {
            from_session(id, &mut curves);
        }
        curves
    }

    fn cache_every_edge(&mut self, document: &Document) {
        let model = document.model();
        let every = &mut self.every_edge;
        every.clear();
        model.entities.for_each(|entity| {
            // What the user can see: a layer turned off does not cut.
            if !is_drawn(model, entity, &NO_LAYER_OVERRIDES) {
                return;
            }
            let mut curves = Vec::new();
            append_edges(&entity.geometry, &mut curves);
            for curve in curves {
                every.push((entity.id, curve));
            }
        });
    }
}

impl InteractiveTool for EdgeTool {
    fn prompt(&self, cx: &ToolContext<'_>) -> String {
        let trimming = self.mode == Mode::Trim;
        if self.step == Step::Edges {
            let count = selection_now(cx, &self.selection, &self.picked).len();
            let what = if trimming {
                "Select cutting edges"
            } else {
                "Select boundary edges"
            };
            if count == 0 {
                return format!("{what} or <all objects>");
            }
            return format!(
                "{what}{} <use {count}{}",
                if self.picked.is_empty() {
                    ""
                } else {
                    " or [Undo]"
                },
                if count == 1 { " edge>" } else { " edges>" }
            );
        }
        format!(
            "{} or [Undo]",
            if trimming {
                "Select object to trim"
            } else {
                "Select object to extend"
            }
        )
    }

    fn expects(&self, _cx: &ToolContext<'_>) -> ToolInput {
        if self.step == Step::Edges {
            ToolInput::Selection
        } else {
            ToolInput::Entity
        }
    }

    fn entity(&mut self, cx: &ToolContext<'_>, id: EntityId, at: Point2) -> ToolStep {
        if self.step == Step::Edges {
            if !self.picked.contains(&id) {
                self.picked.push(id);
            }
            return ToolStep::next("");
        }
        self.pick_target(cx, id, at)
    }

    fn value(&mut self, cx: &ToolContext<'_>, text: &str) -> ToolStep {
        if is_option(text, "Undo", "U") {
            return self.undo(cx);
        }
        if self.step == Step::Edges {
            return ToolStep::rejected(if self.mode == Mode::Trim {
                "Pick the cutting edges and press Enter, or press Enter to cut with every object."
            } else {
                "Pick the boundary edges and press Enter, or press Enter to extend to every \
                 object."
            });
        }
        ToolStep::rejected(if self.mode == Mode::Trim {
            "Pick the part of an object to trim away, or press Enter to finish."
        } else {
            "Pick an object near the end to extend, or press Enter to finish."
        })
    }

    // Esc keeps what the session has done: its Enter only ever commits.
    fn cancel(&mut self, cx: &ToolContext<'_>) -> ToolStep {
        keep_work_on_escape(self, cx)
    }

    fn enter(&mut self, cx: &ToolContext<'_>) -> ToolStep {
        let trimming = self.mode == Mode::Trim;
        if self.step == Step::Edges {
            self.edges = selection_now(cx, &self.selection, &self.picked);
            self.all = self.edges.is_empty();
            if self.all
                && let Some(document) = cx.document
            {
                self.cache_every_edge(document);
            }
            self.step = Step::Targets;
            return ToolStep::next(if self.all {
                String::from("Every object is an edge.")
            } else {
                format!(
                    "{}{}",
                    self.edges.len(),
                    if self.edges.len() == 1 {
                        " edge."
                    } else {
                        " edges."
                    }
                )
            });
        }
        let count = self.session.operations();
        if count == 0 {
            return ToolStep::done(None, "", false);
        }
        let noun = match (trimming, count == 1) {
            (true, true) => " part",
            (true, false) => " parts",
            (false, true) => " end",
            (false, false) => " ends",
        };
        ToolStep::done(
            Some(
                self.session
                    .commit(if trimming { "TRIM" } else { "EXTEND" }),
            ),
            format!(
                "{}{count}{noun}.",
                if trimming { "Trimmed " } else { "Extended " }
            ),
            false,
        )
    }

    fn undo(&mut self, _cx: &ToolContext<'_>) -> ToolStep {
        if self.step == Step::Edges {
            if self.picked.pop().is_none() {
                return ToolStep::rejected("Nothing to undo: no edge has been picked.");
            }
            return ToolStep::next("");
        }
        if self.session.undo() {
            self.marks.pop();
            return ToolStep::next(if self.mode == Mode::Trim {
                "Undid the last trim."
            } else {
                "Undid the last extend."
            });
        }
        // Nothing trimmed yet: step back to choosing the edges.
        self.step = Step::Edges;
        self.all = false;
        self.edges.clear();
        self.every_edge.clear();
        ToolStep::next("")
    }

    fn preview(&self, cx: &ToolContext<'_>, cursor: Point2) -> ToolFeedback {
        let mut feedback = ToolFeedback::default();
        let Some(document) = cx.document else {
            return feedback;
        };
        if self.step == Step::Edges {
            // The edges chosen so far, so the user sees what will cut.
            for id in selection_now(cx, &self.selection, &self.picked) {
                if let Some(entity) = self.session.original(document, id) {
                    feedback.shapes.push(entity.geometry.clone());
                }
            }
            return feedback;
        }
        for mark in &self.marks {
            feedback.shapes.extend(mark.iter().cloned());
        }
        // What a click here would do, found as the view would pick it.
        let under = pick_entity(
            document.model(),
            cursor,
            cpp::max(cx.pick_tolerance, 0.0),
            &SelectionFilter::default(),
            Some(document.spatial_index()),
        );
        if let Some(under) = under
            && let Ok(outcome) = self.attempt(cx, document, under, cursor)
        {
            feedback.shapes.extend(outcome.mark);
        }
        feedback
    }
}

pub fn make_trim_tool(start: &ToolStart) -> ToolPtr {
    Box::new(EdgeTool::new(start, Mode::Trim))
}

pub fn make_extend_tool(start: &ToolStart) -> ToolPtr {
    Box::new(EdgeTool::new(start, Mode::Extend))
}
