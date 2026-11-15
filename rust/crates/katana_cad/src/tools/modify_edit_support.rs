//! What the Modify > Edit tools share (src/katana_cad/tools/modify_edit_support.hpp,
//! namespace tools::modify_edit) - Trim, Extend, Offset, Fillet, Chamfer,
//! Break, Break at Point, Join, Explode (see modify_edit.rs): a session that
//! holds what a tool has changed so far without touching the document, the
//! heights a 3D string carries through an edit, a polyline measured along its
//! length, and the typed-option conventions. In a family module because the
//! other families are written at the same time and may well have helpers of
//! the same names.
//!
//! The C++ EditSession keeps the `const Document*` of the tool's context;
//! here, as every tool is lent its document per call (interactive_tool.rs,
//! "The Rust shape of a tool"), the methods that read the drawing are handed
//! the document instead, and selection_now is handed the selection the tool
//! started with, which the C++ read from its copy of the context.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::Arc;

use katana_commands::{ChangeSet, ChangeSetCommand, CommandPtr};
use katana_core::text::{equals_ignoring_case, format_exact_real, parse_finite_double, trimmed};
use katana_core::{Error, ErrorCode, Status};
use katana_entity::entity::{INVALID_ENTITY_ID, heights_of, set_heights, type_of};
use katana_entity::entity_geometry::distance_to;
use katana_entity::{Entity, EntityId, EntityType, Geometry, validate};
use katana_geometry::{Curve2, CurveSegment, Point2, Polyline2, Segment2};
use katana_math::cpp;
use katana_math::numerics::tolerance;

use crate::document::Document;
use crate::interactive_tool::{
    InteractiveTool, ToolContext, ToolFactory, ToolInput, ToolStart, ToolStep,
};

pub use crate::tools::modify_edit_break::{make_break_at_point_tool, make_break_tool};
pub use crate::tools::modify_edit_corner::{make_chamfer_tool, make_fillet_tool};
pub use crate::tools::modify_edit_join::{make_explode_tool, make_join_tool};
pub use crate::tools::modify_edit_offset::make_offset_tool;
pub use crate::tools::modify_edit_trim::{make_extend_tool, make_trim_tool};

/// What the tools remember from one use to the next, as AutoCAD remembers
/// FILLETRAD, CHAMFERA/CHAMFERB and OFFSETDIST: a user who fillets with
/// radius 5 expects the next fillet to be radius 5 without typing it again.
/// One object per program, shared by the tools' factories (modify_edit.rs);
/// nothing is stored in the drawing.
#[derive(Clone, Debug, PartialEq)]
pub struct EditDefaults {
    pub fillet_radius: f64,
    pub chamfer_first: f64,
    pub chamfer_second: f64,
    /// None: Through, which is also AutoCAD's default before a distance is
    /// typed.
    pub offset_distance: Option<f64>,
    /// How far apart two ends may be and still be joined. A millimetre in
    /// metres: survey linework digitised or imported from another package
    /// often misses by less than that, and a user should not have to snap
    /// every end.
    pub join_tolerance: f64,
}

impl Default for EditDefaults {
    fn default() -> Self {
        Self {
            fillet_radius: 0.0,
            chamfer_first: 0.0,
            chamfer_second: 0.0,
            offset_distance: None,
            join_tolerance: 0.001,
        }
    }
}

/// The program's one set of remembered values, shared by the tools that
/// read them (the C++ `std::shared_ptr<EditDefaults>`). The program runs its
/// tools on one thread, so the set is that thread's (cad_plan.md section 7,
/// risk 4): Rust runs a test binary's cases on threads of their own, where
/// gtest gave each case a process of its own.
pub type Defaults = Rc<RefCell<EditDefaults>>;

/// The C++ `ToolPtr`.
pub type ToolPtr = Box<dyn InteractiveTool>;

/// A factory from a closure, its signature fixed by the bound so that it
/// takes a context and a start of any lifetime, as ToolInfo::make must.
pub fn tool_factory<F>(make: F) -> ToolFactory
where
    F: Fn(&ToolContext<'_>, &ToolStart) -> ToolPtr + Send + Sync + 'static,
{
    Arc::new(make)
}

// ---- typed input -------------------------------------------------------------------

/// core's trimmed over text (the C++ `core::trimmed` of a string_view): it
/// drops ASCII bytes only, so the rest is UTF-8.
pub fn trimmed_text(text: &str) -> &str {
    std::str::from_utf8(trimmed(text.as_bytes())).unwrap_or_default()
}

/// True when `text` is the option `word` or its short form, in any case:
/// "R", "r" and "radius" all choose [Radius].
pub fn is_option(text: &str, word: &str, short_form: &str) -> bool {
    let typed = trimmed(text.as_bytes());
    equals_ignoring_case(typed, word.as_bytes())
        || equals_ignoring_case(typed, short_form.as_bytes())
}

/// A typed number, through core's text so the decimal point is always '.'.
pub fn parse_number(text: &str) -> Option<f64> {
    parse_finite_double(trimmed(text.as_bytes()))
}

// The digits of a shortest scientific mantissa ("-1.5") written out in fixed
// form for `exponent` - what std::to_chars writes with chars_format::fixed.
fn fixed_from_scientific(mantissa: &str, exponent: i32) -> String {
    let (sign, mantissa) = match mantissa.strip_prefix('-') {
        Some(unsigned) => ("-", unsigned),
        None => ("", mantissa),
    };
    let digits = mantissa.replace('.', "");
    let decimal_at = exponent + 1;
    if decimal_at <= 0 {
        format!("{sign}0.{}{digits}", "0".repeat((-decimal_at) as usize))
    } else if decimal_at as usize >= digits.len() {
        format!(
            "{sign}{digits}{}",
            "0".repeat(decimal_at as usize - digits.len())
        )
    } else {
        let (whole, fraction) = digits.split_at(decimal_at as usize);
        format!("{sign}{whole}.{fraction}")
    }
}

/// A number as a prompt shows it: the shortest text that reads back exactly.
pub fn format_number(value: f64) -> String {
    // The shortest text that reads back exactly, as format_exact_real writes
    // - but never in exponent form, which a person reading "within 1e-04" in
    // a prompt should not have to decode. Beyond 1e15 a fixed figure is a
    // wall of digits, and no drawing value is that large, so the exact form
    // stays.
    if !value.is_finite() || value.abs() >= 1e15 {
        return format_exact_real(value);
    }
    // format_exact_real chooses the shorter of the fixed and the scientific
    // form of the same shortest digits (std::to_chars); the fixed one is
    // std::to_chars' chars_format::fixed.
    let exact = format_exact_real(value);
    let Some((mantissa, exponent)) = exact.split_once('e') else {
        return exact;
    };
    match exponent.parse::<i32>() {
        Ok(exponent) => fixed_from_scientific(mantissa, exponent),
        Err(_) => exact,
    }
}

/// An error from a command or the geometry layer as a sentence for the
/// command line: capitalised, with a full stop.
pub fn as_sentence(text: &str) -> String {
    let mut out = String::from(trimmed_text(text));
    if out.is_empty() {
        return out;
    }
    if out.starts_with(|first: char| first.is_ascii_lowercase()) {
        let first = out.remove(0);
        out.insert(0, first.to_ascii_uppercase());
    }
    if !out.ends_with(['.', '?', '!']) {
        out.push('.');
    }
    out
}

// ---- the drawing -------------------------------------------------------------------

/// "line", "arc", "circle", "polyline", "point", "text", "dimension".
pub fn kind_name(geometry: &Geometry) -> &'static str {
    match type_of(geometry) {
        EntityType::Point => "point",
        EntityType::Line => "line",
        EntityType::Arc => "arc",
        EntityType::Polyline => match geometry {
            Geometry::Polyline(polyline) if polyline.closed => "closed polyline",
            _ => "polyline",
        },
        EntityType::Circle => "circle",
        EntityType::Text => "text",
        EntityType::Dimension => "dimension",
        EntityType::Label => "label",
        EntityType::Leader => "leader",
        EntityType::CurvePolyline => match geometry {
            Geometry::CurvePolyline(curved) if curved.closed => "closed polyline",
            _ => "polyline",
        },
        EntityType::Ellipse => match geometry {
            Geometry::Ellipse(ellipse) if !ellipse.is_full() => "elliptical arc",
            _ => "ellipse",
        },
        EntityType::Spline => "spline",
    }
}

/// "a line", "an arc".
pub fn with_article(noun: &str) -> String {
    let vowel = noun.starts_with(['a', 'e', 'i', 'o', 'u']);
    format!("{}{noun}", if vowel { "an " } else { "a " })
}

/// Why `id` cannot be edited - it is not in the drawing, or its layer is
/// locked - or None when it can. A tool refuses the pick up front with this
/// rather than returning a command the document would refuse.
pub fn refusal_to_edit(document: &Document, id: EntityId) -> Option<String> {
    let Some(entity) = document.model().entities.find(id) else {
        return Some(String::from("That object is no longer in the drawing."));
    };
    if document.model().layers.effectively_locked(&entity.layer) {
        return Some(format!(
            "That {} is on the locked layer '{}'; unlock the layer to edit it.",
            kind_name(&entity.geometry),
            entity.layer
        ));
    }
    None
}

/// Whether the drawing would hold `geometry` - the check the command that
/// adds it makes (entity's validate) - as an error whose message is the
/// sentence to refuse with. A tool's whole session becomes one command, so a
/// single piece the drawing refuses would lose every edit made with it; a
/// tool asks this before it takes a pick, and EditSession asks it of
/// everything it is given.
pub fn drawable(geometry: &Geometry) -> Status {
    if let Err(error) = validate(geometry) {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            as_sentence(&format!(
                "that would make {} the drawing cannot hold: {}",
                with_article(kind_name(geometry)),
                error.message
            )),
        ));
    }
    Ok(())
}

/// The entities a Selection step ends with: what the tool was given or
/// picked, and the document's selection as it is when Enter is pressed (the
/// view selects as the Select tool does while a tool asks for a selection).
/// Ascending, without duplicates. `selection` is the selection the tool
/// started with (ToolStart), which stands in for the live one only when the
/// context has no document, as the C++ context's did.
pub fn selection_now(
    cx: &ToolContext<'_>,
    selection: &[EntityId],
    picked: &[EntityId],
) -> Vec<EntityId> {
    let mut ids = picked.to_vec();
    match cx.document {
        // The live selection rather than the one the tool started with: the
        // view changes it while a Selection step is open, and an entity the
        // user deselected there must not be acted on.
        Some(document) => ids.extend(document.selection().ids()),
        None => ids.extend_from_slice(selection),
    }
    ids.sort_unstable();
    ids.dedup();
    if let Some(document) = cx.document {
        ids.retain(|&id| document.model().entities.contains(id));
    }
    ids
}

/// A step refused because the context lends no document. The C++ tools
/// dereference their document unchecked (the plan view always gives one, and
/// ToolDriver does); a Rust tool refuses instead.
pub fn no_drawing() -> ToolStep {
    ToolStep::rejected("There is no drawing to edit.")
}

// ---- the trait's own answers -------------------------------------------------------
//
// A C++ tool that overrides an input calls the base class's for the steps it
// does not take (`return InteractiveTool::point(at);`). A Rust override
// cannot reach the trait's default, so these answer through a tool that
// overrides nothing but expects(): the default text, from its one home.

struct BaseTool(ToolInput);

impl InteractiveTool for BaseTool {
    fn prompt(&self, _cx: &ToolContext<'_>) -> String {
        String::new()
    }

    fn expects(&self, _cx: &ToolContext<'_>) -> ToolInput {
        self.0
    }
}

/// The C++ `InteractiveTool::point(at)` of a tool that `expects` that now.
pub fn base_point(cx: &ToolContext<'_>, expects: ToolInput, at: Point2) -> ToolStep {
    BaseTool(expects).point(cx, at)
}

/// The C++ `InteractiveTool::entity(id, at)` of a tool that `expects` that now.
pub fn base_entity(cx: &ToolContext<'_>, expects: ToolInput, id: EntityId, at: Point2) -> ToolStep {
    BaseTool(expects).entity(cx, id, at)
}

/// The C++ `InteractiveTool::value(text)`.
pub fn base_value(cx: &ToolContext<'_>, text: &str) -> ToolStep {
    BaseTool(ToolInput::Value).value(cx, text)
}

/// The C++ `InteractiveTool::undo()`.
pub fn base_undo(cx: &ToolContext<'_>) -> ToolStep {
    BaseTool(ToolInput::Value).undo(cx)
}

// ---- heights -----------------------------------------------------------------------
//
// A 3D string keeps a height per vertex in its properties (entity.rs:
// set_heights / heights_of). An edit that moves or adds vertices must say
// what the new vertices' heights are, or the list no longer matches the
// geometry and the readers drop it. A vertex that lies on the original gets
// the height interpolated there; one that does not (an extended end) gets
// none, because absent is not zero and a guess is not a survey.

/// The points heights are kept for: a line's and an arc's two ends, a
/// polyline's vertices, a circle's centre, a point's position.
pub fn height_vertices(geometry: &Geometry) -> Vec<Point2> {
    match geometry {
        Geometry::Point(point) => vec![point.position],
        Geometry::Line(segment) => vec![segment.start, segment.end],
        Geometry::Arc(arc) => vec![arc.start_point(), arc.end_point()],
        Geometry::Polyline(polyline) => polyline.vertices.clone(),
        Geometry::Circle(circle) => vec![circle.center],
        Geometry::Text(text) => vec![text.position],
        // A dimension has no heights of its own.
        _ => Vec::new(),
    }
}

// The height a fraction `t` of the way from a vertex of height `a` to one of
// height `b`: that vertex's own at either end, else interpolated when both
// are known. A vertex exactly at an end must not lose its height because the
// other end has none.
fn interpolated(
    a: Option<f64>,
    b: Option<f64>,
    t: f64,
    at_start: bool,
    at_end: bool,
) -> Option<f64> {
    if at_start {
        return a;
    }
    if at_end {
        return b;
    }
    match (a, b) {
        (Some(a), Some(b)) => Some(a + (b - a) * t),
        _ => None,
    }
}

/// The height the original has at `p`, when `p` lies on it and the heights
/// on either side are known.
pub fn height_at(original: &Entity, p: Point2) -> Option<f64> {
    if let Geometry::CurvePolyline(curved) = &original.geometry {
        // Its heights are its vertices' own (docs/drawing.md).
        let nearest = curved.nearest(p)?;
        if nearest.distance > tolerance::GEOMETRIC {
            return None;
        }
        let i = nearest.segment;
        let next = curved.segment_end(i);
        let length = curved.segment_length(i);
        let t = if length > 0.0 {
            (nearest.station - curved.station_of_vertex(i)) / length
        } else {
            0.0
        };
        return interpolated(
            curved.vertices[i].height,
            curved.vertices[next].height,
            t,
            p.distance_to(curved.vertices[i].position) <= tolerance::GEOMETRIC,
            p.distance_to(curved.vertices[next].position) <= tolerance::GEOMETRIC,
        );
    }
    let count = height_vertices(&original.geometry).len();
    if count == 0 {
        return None;
    }
    let heights = heights_of(&original.properties, count);
    match &original.geometry {
        Geometry::Line(segment) => {
            if segment.distance_to(p) > tolerance::GEOMETRIC {
                return None;
            }
            interpolated(
                heights[0],
                heights[1],
                segment.parameter_of(p),
                p.distance_to(segment.start) <= tolerance::GEOMETRIC,
                p.distance_to(segment.end) <= tolerance::GEOMETRIC,
            )
        }
        Geometry::Arc(arc) => {
            if arc.distance_to(p) > tolerance::GEOMETRIC {
                return None;
            }
            interpolated(
                heights[0],
                heights[1],
                arc.parameter_of_angle((p - arc.center).angle()),
                p.distance_to(arc.start_point()) <= tolerance::GEOMETRIC,
                p.distance_to(arc.end_point()) <= tolerance::GEOMETRIC,
            )
        }
        Geometry::Circle(circle) => {
            // A circle is level: its one height is the height of every point
            // on it.
            let on_it = circle.distance_to(p) <= tolerance::GEOMETRIC
                || circle.center.distance_to(p) <= tolerance::GEOMETRIC;
            if on_it { heights[0] } else { None }
        }
        Geometry::Polyline(polyline) => {
            let mut best: Option<usize> = None;
            let mut best_distance = f64::INFINITY;
            for i in 0..polyline.segment_count() {
                let d = polyline.segment(i).distance_to(p);
                if d < best_distance {
                    best_distance = d;
                    best = Some(i);
                }
            }
            let best = best?;
            if best_distance > tolerance::GEOMETRIC {
                return None;
            }
            let segment = polyline.segment(best);
            let next = (best + 1) % polyline.vertices.len();
            interpolated(
                heights[best],
                heights[next],
                segment.parameter_of(p),
                p.distance_to(segment.start) <= tolerance::GEOMETRIC,
                p.distance_to(segment.end) <= tolerance::GEOMETRIC,
            )
        }
        Geometry::Point(point) => {
            if point.position.distance_to(p) <= tolerance::GEOMETRIC {
                heights[0]
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Sets `piece`'s heights from where its vertices lie on `original`.
pub fn carry_heights(original: &Entity, piece: &mut Entity) {
    let heights: Vec<Option<f64>> = height_vertices(&piece.geometry)
        .into_iter()
        .map(|vertex| height_at(original, vertex))
        .collect();
    set_heights(&mut piece.properties, &heights);
}

// ---- curves ------------------------------------------------------------------------

/// The curves an entity cuts or bounds with: a line, arc or circle is itself,
/// a polyline is its segments, anything else is nothing.
pub fn append_edges(geometry: &Geometry, out: &mut Vec<Curve2>) {
    match geometry {
        Geometry::Polyline(polyline) => {
            for i in 0..polyline.segment_count() {
                let segment = polyline.segment(i);
                if !segment.is_degenerate() {
                    out.push(Curve2::Segment(segment));
                }
            }
        }
        Geometry::CurvePolyline(curved) => {
            for piece in curved.segments() {
                if piece.length() > tolerance::GEOMETRIC {
                    out.push(match piece {
                        CurveSegment::Segment(segment) => Curve2::Segment(segment),
                        CurveSegment::Arc(arc) => Curve2::Arc(arc),
                    });
                }
            }
        }
        _ => {
            if let Some(curve) = as_curve(geometry) {
                out.push(curve);
            }
        }
    }
}

pub fn as_geometry(curve: &Curve2) -> Geometry {
    match *curve {
        Curve2::Segment(segment) => Geometry::Line(segment),
        Curve2::Arc(arc) => Geometry::Arc(arc),
        Curve2::Circle(circle) => Geometry::Circle(circle),
    }
}

pub fn as_curve(geometry: &Geometry) -> Option<Curve2> {
    match geometry {
        Geometry::Line(segment) => Some(Curve2::Segment(*segment)),
        Geometry::Arc(arc) => Some(Curve2::Arc(*arc)),
        Geometry::Circle(circle) => Some(Curve2::Circle(*circle)),
        _ => None,
    }
}

// ---- PolylinePath ------------------------------------------------------------------

/// A polyline measured along its length ("stations", from its first vertex),
/// so a break or a trim can say "from here to there" across vertices.
#[derive(Clone, Debug, PartialEq)]
pub struct PolylinePath {
    polyline: Polyline2,
    /// The vertices, and the first again when closed.
    path: Vec<Point2>,
    /// The station of each of `path`.
    stations: Vec<f64>,
}

fn drop_repeats(vertices: &mut Vec<Point2>) {
    vertices.dedup_by(|current, kept| kept.distance_to(*current) <= tolerance::GEOMETRIC);
}

impl PolylinePath {
    pub fn new(polyline: Polyline2) -> Self {
        let mut path = polyline.vertices.clone();
        if polyline.closed && !path.is_empty() {
            path.push(path[0]);
        }
        let mut stations = vec![0.0];
        for pair in path.windows(2) {
            let previous = stations[stations.len() - 1];
            stations.push(previous + pair[0].distance_to(pair[1]));
        }
        Self {
            polyline,
            path,
            stations,
        }
    }

    pub fn polyline(&self) -> &Polyline2 {
        &self.polyline
    }

    pub fn closed(&self) -> bool {
        self.polyline.closed
    }

    /// Including the closing segment of a closed polyline.
    pub fn length(&self) -> f64 {
        self.stations[self.stations.len() - 1]
    }

    /// The station of the point of the path nearest to `p`.
    pub fn station_of(&self, p: Point2) -> f64 {
        let mut best = 0;
        let mut best_distance = f64::INFINITY;
        for i in 0..self.path.len().saturating_sub(1) {
            let d = self.segment(i).distance_to(p);
            if d < best_distance {
                best_distance = d;
                best = i;
            }
        }
        if self.path.len() < 2 {
            0.0
        } else {
            self.station_on(best, p)
        }
    }

    /// The station of `point_on_segment`, a point of segment `segment_index`
    /// (the C++ `stationOf(segment, point)` overload).
    pub fn station_on(&self, segment_index: usize, point_on_segment: Point2) -> f64 {
        let piece = self.segment(segment_index);
        self.stations[segment_index] + piece.parameter_of(point_on_segment) * piece.length()
    }

    pub fn point_at(&self, station: f64) -> Point2 {
        if self.path.is_empty() {
            return Point2::default();
        }
        let s = cpp::clamp(station, 0.0, self.length());
        for i in 0..self.path.len() - 1 {
            if s > self.stations[i + 1] + tolerance::GEOMETRIC {
                continue;
            }
            // A station at a vertex returns the vertex itself, not a sum that
            // rounds a last bit away from it.
            if (s - self.stations[i]).abs() <= tolerance::GEOMETRIC {
                return self.path[i];
            }
            if (s - self.stations[i + 1]).abs() <= tolerance::GEOMETRIC {
                return self.path[i + 1];
            }
            let span = self.stations[i + 1] - self.stations[i];
            return self.segment(i).point_at(if span > 0.0 {
                (s - self.stations[i]) / span
            } else {
                0.0
            });
        }
        self.path[self.path.len() - 1]
    }

    pub fn segment_count(&self) -> usize {
        // The C++ subtracts from an unsigned size; a path of no vertex has no
        // segment either way.
        self.path.len().saturating_sub(1)
    }

    pub fn segment(&self, index: usize) -> Segment2 {
        Segment2::new(self.path[index], self.path[index + 1])
    }

    /// The open polyline from station `from` to `to` (from < to), keeping
    /// every vertex between.
    pub fn between(&self, from: f64, to: f64) -> Polyline2 {
        let mut out = Polyline2::new(vec![self.point_at(from)], false);
        for (&station, &vertex) in self.stations.iter().zip(&self.path) {
            if station > from + tolerance::GEOMETRIC && station < to - tolerance::GEOMETRIC {
                out.vertices.push(vertex);
            }
        }
        out.vertices.push(self.point_at(to));
        drop_repeats(&mut out.vertices);
        out
    }

    /// Closed paths: from `from` forward through the first vertex to `to`.
    pub fn wrapping(&self, from: f64, to: f64) -> Polyline2 {
        let mut out = self.between(from, self.length());
        let tail = self.between(0.0, to);
        out.vertices.extend(tail.vertices);
        drop_repeats(&mut out.vertices);
        out
    }
}

// ---- the session -------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq)]
struct SessionState {
    edited: BTreeMap<EntityId, Vec<Entity>>,
    added: Vec<Entity>,
}

/// What a tool has changed so far, as a layer over the document. Trim takes
/// several picks, and a later pick can land on a piece an earlier one made -
/// which has no id until a command runs - so the tool edits this instead, and
/// when it finishes turns the whole session into ONE command: one undo takes
/// back everything the tool did, as AutoCAD's U does after a TRIM.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EditSession {
    state: SessionState,
    history: Vec<SessionState>,
}

impl EditSession {
    pub fn new() -> Self {
        Self::default()
    }

    /// The entity as the document has it, or None.
    pub fn original<'d>(&self, document: &'d Document, id: EntityId) -> Option<&'d Entity> {
        document.model().entities.find(id)
    }

    /// What the session has made of `id`: the document's entity until it is
    /// edited, then its pieces (the first keeps the id); empty once removed.
    pub fn pieces(&self, document: &Document, id: EntityId) -> Vec<Entity> {
        if let Some(found) = self.state.edited.get(&id) {
            return found.clone();
        }
        match self.original(document, id) {
            Some(entity) => vec![entity.clone()],
            None => Vec::new(),
        }
    }

    /// New entities (offsets, fillet arcs), in the order made.
    pub fn added(&self) -> &[Entity] {
        &self.state.added
    }

    pub fn edited(&self, id: EntityId) -> bool {
        self.state.edited.contains_key(&id)
    }

    pub fn edited_ids(&self) -> Vec<EntityId> {
        self.state.edited.keys().copied().collect()
    }

    /// Everything the session has made, for a preview: the pieces of every
    /// edited entity and every added one.
    pub fn shapes(&self) -> Vec<Geometry> {
        let mut out = Vec::new();
        for pieces in self.state.edited.values() {
            for piece in pieces {
                out.push(piece.geometry.clone());
            }
        }
        for entity in &self.state.added {
            out.push(entity.geometry.clone());
        }
        out
    }

    /// Marks the start of one operation, which undo() takes back whole.
    pub fn begin(&mut self) {
        self.history.push(self.state.clone());
    }

    /// Fails, with drawable()'s sentence, when a piece is one the drawing
    /// would refuse - and then the whole operation since begin() is
    /// abandoned, so an operation is taken entirely or not at all and the
    /// command the session becomes is always one the drawing accepts.
    pub fn replace(&mut self, id: EntityId, pieces: Vec<Entity>) -> Status {
        for piece in &pieces {
            if let Err(error) = drawable(&piece.geometry) {
                self.undo(); // abandon the operation begun: all of it or none
                return Err(error);
            }
        }
        self.state.edited.insert(id, pieces);
        Ok(())
    }

    /// As replace(), for a new entity.
    pub fn add(&mut self, mut entity: Entity) -> Status {
        if let Err(error) = drawable(&entity.geometry) {
            self.undo();
            return Err(error);
        }
        entity.id = INVALID_ENTITY_ID;
        self.state.added.push(entity);
        Ok(())
    }

    /// Back to before the last begin(). False when there is nothing to undo.
    pub fn undo(&mut self) -> bool {
        match self.history.pop() {
            Some(previous) => {
                self.state = previous;
                true
            }
            None => false,
        }
    }

    pub fn operations(&self) -> usize {
        self.history.len()
    }

    /// Everything as one command: pieces[0] modifies its entity, further
    /// pieces and added entities are created, an emptied entity is removed.
    pub fn commit(&self, name: &str) -> CommandPtr {
        let mut changes = ChangeSet::default();
        for (&id, pieces) in &self.state.edited {
            let Some((first, rest)) = pieces.split_first() else {
                changes.remove.push(id);
                continue;
            };
            let mut first = first.clone();
            first.id = id;
            changes.modify.push(first);
            for piece in rest {
                let mut piece = piece.clone();
                piece.id = INVALID_ENTITY_ID;
                changes.add.push(piece);
            }
        }
        changes.add.extend(self.state.added.iter().cloned());
        // The set is worked out now, from the drawing the user saw while
        // picking, and handed over whole: the command's validation still
        // checks it against the model it is applied to.
        Box::new(ChangeSetCommand::new(
            name,
            move |_context| Ok(changes.clone()),
            false,
        ))
    }
}

/// The piece of `pieces` nearest to `at`, or None when there are none.
pub fn nearest_piece(pieces: &[Entity], at: Point2) -> Option<usize> {
    let mut best = None;
    let mut best_distance = f64::INFINITY;
    for (i, piece) in pieces.iter().enumerate() {
        let d = distance_to(&piece.geometry, at);
        if d < best_distance {
            best_distance = d;
            best = Some(i);
        }
    }
    best
}
