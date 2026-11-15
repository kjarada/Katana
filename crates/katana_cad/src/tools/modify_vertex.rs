//! The Draw > Vertices tools (src/katana_cad/tools/modify_vertex.cpp;
//! tools/families.rs): vertex control of polylines, the owner's request of
//! 2026-09-25, as catalogue tools - insert, delete and move a vertex,
//! straighten, weed, densify, close or open, change the start, heights,
//! segments to arcs and back, fillet and chamfer one corner, merge near
//! vertices, snap to the grid - and Edit Vertices, which hands a polyline to
//! the Vertices panel.
//!
//! Every tool is a short SCRIPT of steps - pick a vertex, a segment or a
//! polyline, click a point on a polyline, a point, a value, or several
//! polylines - run by one state machine here, and ends in ONE command over
//! the arithmetic of geometry's polyline_vertices. So the tools, the grips,
//! the panel and the VERTEX verbs cannot disagree about what an edit does:
//! the tools only collect its arguments. A tool that works on one pick
//! restarts, as Point and Circle do, for a run of edits; each edit is its own
//! undo step.
//!
//! What a pick takes, and what a tool shows before the click, are the rules
//! of docs/drawing.md, "What a vertex tool acts on" - the owner's request of
//! 2026-09-30, that inserting a vertex gave "no visual clue where the vertex
//! is going" and that a tool asked for a polyline "but what about the already
//! selected vertex":
//!
//! * ONE function, resolve(), decides what a pick takes, for the preview,
//!   the click and a typed point alike: the grips hot when the tool started
//!   (its handles), else the nearest SELECTED polyline within reach, else the
//!   polyline under the cursor - never another kind of entity, so a survey
//!   point on a vertex cannot take the pick from its string;
//! * a vertex is picked within the view's 12 px, a segment or a point on a
//!   polyline within its 8 px: never "the nearest vertex" at any distance. A
//!   typed x,y at a pick is taken as a click there is, within the same reach,
//!   and one that misses says so (a typed number names a vertex at any
//!   zoom). What a place given exactly - typed, or Enter's marked middle - is
//!   spared is the rule about what a POINTER meant, Insert's "too close to
//!   vertex N": there only geometry's own refusal, a point on a vertex,
//!   applies (Chosen::exact);
//! * each one-polyline tool PLANS its edit with the geometry call the commit
//!   makes, and the preview draws the plan by role (tool_feedback.rs): what a
//!   click takes, adds and removes. A plan refused for the pick itself (an
//!   end vertex for Fillet) is refused at the pick, before a value is asked;
//! * an edit is made from the polyline as it was picked, and one changed
//!   since is refused, never silently reverted. What a tool holds - its
//!   picks, the grips chosen before it, Insert's anchor - is checked again at
//!   every preview and every input (why_not_now): changed, deleted, hidden or
//!   locked since, it is shown refused and the next input drops it.
//!
//! The Rust shape (interactive_tool.rs, "The Rust shape of a tool"): the
//! C++'s Collected kept the tool's `const Document*`; here nothing collected
//! holds the document, and every plan, prompt and pick is lent it per call,
//! through the context or as `Option<&Document>`. What the handles answer is
//! read from the ToolStart (its `handles` and `selection`), where the C++
//! read them from its context. What the tools remember between uses
//! (VertexDefaults) is per thread (the cad plan, section 7, risk 4).

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use katana_commands::CommandPtr;
use katana_core::text::{
    equals_ignoring_case, format_exact_real, parse_finite_double, parse_integer,
};
use katana_core::{Error, ErrorCode};
use katana_entity::entity::INVALID_ENTITY_ID;
use katana_entity::{EntityId, EntityType, Geometry, Model, PointGeometry};
use katana_geometry::curves2d::arc_from_bulge;
use katana_geometry::polyline_vertices::{
    chamfer_vertex, change_start_vertex, check_corner, close_polyline, delete_vertex,
    delete_vertices, densify, fillet_vertex, grade_between, insert_vertex, interpolate_heights,
    merge_near_vertices, move_vertex, nearest_segment, nearest_vertex, open_polyline,
    segment_to_arc, segment_to_line, set_vertex_height, snap_to_grid, straighten, vertices_between,
    weed,
};
use katana_geometry::{CurvePolyline2, CurveSegment, Point2, Polyline2, PolylineResult, Segment2};
use katana_math::cpp;
use katana_math::numerics::tolerance;

use crate::document::Document;
use crate::drawing::drafting::format_bearing;
use crate::drawing::grips::{Grip, GripKind};
use crate::drawing::vertex_editing::{
    SURVEY_POINT_TOLERANCE, edit_each_polyline, edit_polylines, height_at_point, height_text,
    is_polyline_entity, read_polyline, vertex_range, vertices_on_survey_points,
};
use crate::interactive_tool::{
    InteractiveTool, PICK_APERTURE_PIXELS, SNAP_APERTURE_PIXELS, ToolCatalog, ToolContext,
    ToolInfo, ToolInput, ToolStart, ToolStep, ToolStepOutcome, pick_reach, pick_under, pickable,
    vertex_reach, view_of,
};
use crate::selection::is_drawn;
use crate::snapping::SnapResult;
use crate::tool_feedback::{FeedbackMark, FeedbackRole, ToolFeedback};
use crate::tools::annotate_common::{default_point, factory, trimmed};
use crate::tools::everyday_support::fixed;
use crate::tools::families::Report;

// Only polylines are picked: a survey point or a text on a vertex - where the
// prompts send the cursor - must not take the pick from the string.
fn polyline_kinds() -> BTreeSet<EntityType> {
    BTreeSet::from([EntityType::Polyline, EntityType::CurvePolyline])
}

// What the tools remember between uses, as a CAD program keeps the last
// fillet radius: one object for the program, nothing of it in the drawing.
#[derive(Clone, Copy, Debug, PartialEq)]
struct VertexDefaults {
    weed_tolerance: f64,
    weed_keeps_survey_points: bool,
    densify_interval: f64,
    densify_chord: f64,
    fillet_radius: f64,
    chamfer_first: f64,
    chamfer_second: f64,
    merge_tolerance: f64,
    grid_spacing: f64,
}

impl VertexDefaults {
    const INITIAL: Self = Self {
        weed_tolerance: 0.01,
        weed_keeps_survey_points: true,
        densify_interval: 1.0,
        densify_chord: 0.0,
        fillet_radius: 1.0,
        chamfer_first: 1.0,
        chamfer_second: 1.0,
        merge_tolerance: 0.001,
        grid_spacing: 1.0,
    };
}

thread_local! {
    // The program uses the tools on one thread; a test thread has its own.
    static DEFAULTS: Cell<VertexDefaults> = const { Cell::new(VertexDefaults::INITIAL) };
}

fn defaults() -> VertexDefaults {
    DEFAULTS.with(Cell::get)
}

fn keep_defaults(change: impl FnOnce(&mut VertexDefaults)) {
    DEFAULTS.with(|kept| {
        let mut now = kept.get();
        change(&mut now);
        kept.set(now);
    });
}

fn is_option(text: &str, keyword: &str) -> bool {
    let typed = trimmed(text);
    !typed.is_empty()
        && typed.len() <= keyword.len()
        && equals_ignoring_case(typed.as_bytes(), &keyword.as_bytes()[..typed.len()])
}

fn number(value: f64) -> String {
    format_exact_real(value)
}

fn plural(count: usize, noun: &str) -> String {
    if noun == "vertex" {
        return format!(
            "{count}{}",
            if count == 1 { " vertex" } else { " vertices" }
        );
    }
    format!("{count} {noun}{}", if count == 1 { "" } else { "s" })
}

// A whole number typed at a pick: vertex (or segment) N of the polyline in
// play, read as the drawing verbs read an index (drawing_verbs.cpp,
// indexOf): core's parse_integer takes the whole token, so "1.5" is refused
// rather than rounded, and a negative one is no index.
fn whole_number(text: &str) -> Option<usize> {
    let value = parse_integer(text)?;
    if value < 0 {
        return None;
    }
    Some(value as usize)
}

fn parse_positive(text: &str) -> Option<f64> {
    parse_finite_double(trimmed(text)).filter(|&value| value > 0.0)
}

fn parse_non_negative(text: &str) -> Option<f64> {
    parse_finite_double(trimmed(text)).filter(|&value| value >= 0.0)
}

// ---- pieces of a polyline -------------------------------------------------------------------

fn piece_of(shape: &CurvePolyline2, segment: usize) -> Geometry {
    match shape.segment(segment) {
        CurveSegment::Segment(piece) => Geometry::Line(piece),
        CurveSegment::Arc(piece) => Geometry::Arc(piece),
    }
}

fn whole_of(shape: &CurvePolyline2) -> Geometry {
    if shape.has_arcs() {
        return Geometry::CurvePolyline(shape.clone());
    }
    Geometry::Polyline(Polyline2::new(shape.positions(), shape.closed))
}

fn distance_to_piece(shape: &CurvePolyline2, segment: usize, at: Point2) -> f64 {
    match shape.segment(segment) {
        CurveSegment::Segment(piece) => piece.distance_to(at),
        CurveSegment::Arc(piece) => piece.distance_to(at),
    }
}

fn closest_on_piece(shape: &CurvePolyline2, segment: usize, at: Point2) -> Point2 {
    match shape.segment(segment) {
        CurveSegment::Segment(piece) => piece.closest_point(at),
        CurveSegment::Arc(piece) => piece.closest_point(at),
    }
}

fn middle_of_piece(shape: &CurvePolyline2, segment: usize) -> Point2 {
    shape.segment(segment).point_at(0.5)
}

// The segment that ends at vertex `v`, and the one that starts at it, where
// the polyline has them.
fn segment_before(shape: &CurvePolyline2, v: usize) -> Option<usize> {
    if v > 0 {
        return Some(v - 1);
    }
    if shape.closed && shape.vertices.len() > 1 {
        return Some(shape.vertices.len() - 1);
    }
    None
}

fn segment_after(shape: &CurvePolyline2, v: usize) -> Option<usize> {
    (v < shape.segment_count()).then_some(v)
}

// The segments walked forward from vertex `from` to vertex `to`.
fn segments_between(shape: &CurvePolyline2, from: usize, to: usize) -> Vec<usize> {
    let mut out = Vec::new();
    let n = shape.vertices.len();
    let mut i = from;
    while i != to && out.len() < n {
        out.push(i);
        i = (i + 1) % n;
    }
    out
}

fn vertex_mark(role: FeedbackRole, at: Point2, label: impl Into<String>) -> FeedbackMark {
    FeedbackMark {
        role,
        geometry: Geometry::Point(PointGeometry { position: at }),
        label: label.into(),
        refused: false,
    }
}

fn piece_mark(role: FeedbackRole, piece: Geometry, label: impl Into<String>) -> FeedbackMark {
    FeedbackMark {
        role,
        geometry: piece,
        label: label.into(),
        refused: false,
    }
}

// ---- what a pick chose, and what the tool will do with it ------------------------------------

// What one pick chose, kept as it was chosen - the polyline's shape then
// included - so the plan and the edit work from what the user saw, and the
// edit can tell that the polyline has changed since (stale()).
#[derive(Clone, Debug, Default, PartialEq)]
struct Chosen {
    polyline: EntityId,
    shape: CurvePolyline2,
    vertex: Option<usize>,
    segment: Option<usize>,
    // The vertex, or where on the segment or the polyline.
    at: Point2,
    // A grip made hot before the tool, not a click.
    #[allow(
        dead_code,
        reason = "Kept as the C++ keeps it, set wherever a pick is a grip chosen before the tool; nothing reads it yet."
    )]
    from_handle: bool,
    // Given exactly - Enter's marked middle, a typed number or point - not
    // pointed at: a rule in pixels about what a pointer meant (Insert's "too
    // close to vertex N") does not apply, only geometry's own.
    exact: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Collected {
    // One per pick step, except that Delete's hot vertices are several picks
    // answering its one step.
    picks: Vec<Chosen>,
    points: Vec<Point2>,
    // The z of a point typed as x,y,z: Insert's new vertex's height, Move's.
    height: Option<f64>,
    values: Vec<String>,
    selection: Vec<EntityId>,
    // Straighten and Grade on a closed polyline: the side typed O chose.
    other_side: bool,
    // Insert: the hot vertex or segment the new vertex goes beside.
    anchor: Option<Chosen>,
    // Said before the first prompt: why what was chosen before the tool is
    // not what it uses - "2 vertices are chosen", an end vertex for Fillet,
    // a selected polyline on a locked layer. A script's answer() gives the
    // reason; the tool adds the prompt.
    note: String,
}

// Insert beside a hot handle: the handle's segment, or whichever of a hot
// vertex's two segments is nearer `at` - at any distance, since the user
// chose it - with the point put on that segment. None where a click takes
// nothing, within `reach` (0 for a typed point, which is exact): ON the
// chosen vertex, where the pointer rests after choosing it, and PAST the
// segment's ends, where the point put on it is an end the cursor is not
// near. Both were "too close to vertex N; zoom in", at any distance and at
// any zoom - the first thing the tool showed after a vertex was chosen.
fn beside_anchor(anchor: &Chosen, at: Point2, reach: f64) -> Option<Chosen> {
    if let Some(vertex) = anchor.vertex
        && at.distance_to(anchor.shape.vertices[vertex].position) <= reach
    {
        return None;
    }
    let mut chosen = anchor.clone();
    chosen.from_handle = true;
    let mut segment = anchor.segment.unwrap_or(0);
    if let Some(vertex) = anchor.vertex {
        let before = segment_before(&anchor.shape, vertex);
        let after = segment_after(&anchor.shape, vertex);
        segment = match (before, after) {
            (Some(before), Some(after)) => {
                if distance_to_piece(&anchor.shape, before, at)
                    < distance_to_piece(&anchor.shape, after, at)
                {
                    before
                } else {
                    after
                }
            }
            _ => before.or(after).unwrap_or(0),
        };
    }
    chosen.vertex = None;
    chosen.segment = Some(segment);
    chosen.at = closest_on_piece(&anchor.shape, segment, at);
    for end in [segment, anchor.shape.segment_end(segment)] {
        let corner = anchor.shape.vertices[end].position;
        if chosen.at.distance_to(corner) <= tolerance::GEOMETRIC && at.distance_to(corner) > reach {
            return None;
        }
    }
    Some(chosen)
}

// Enter beside a hot handle: the middle of the handle's segment, or of the
// segment after a hot vertex (before it, at an open polyline's end). Exact:
// the place is named, not pointed at, so it is taken on a segment however
// few pixels long (Chosen::exact).
fn middle_beside(anchor: &Chosen) -> Chosen {
    let mut chosen = anchor.clone();
    chosen.from_handle = true;
    chosen.exact = true;
    let mut segment = anchor.segment.unwrap_or(0);
    if let Some(vertex) = anchor.vertex {
        segment = segment_after(&anchor.shape, vertex)
            .or_else(|| segment_before(&anchor.shape, vertex))
            .unwrap_or(0);
    }
    chosen.vertex = None;
    chosen.segment = Some(segment);
    chosen.at = middle_of_piece(&anchor.shape, segment);
    chosen
}

// An edit of one polyline, from the shape it was picked as.
#[derive(Clone, Debug, Default, PartialEq)]
struct Edit {
    polyline: EntityId,
    before: CurvePolyline2,
    after: CurvePolyline2,
}

// What a tool would do with what it has collected - the one answer the
// preview draws and the commit executes (docs/drawing.md, R3).
#[derive(Clone, Debug, Default, PartialEq)]
struct Plan {
    // The pick (or point) cannot be taken: said in the preview, and a click
    // there is refused with it.
    refusal: Option<String>,
    // The edit fails at a value's default (a radius that does not fit): said
    // in the preview, but the pick is taken and the value asked for.
    warning: Option<String>,
    // The undo name: VERTEX_INSERT, STRAIGHTEN, ...
    command: String,
    edits: Vec<Edit>,
    // Edit Vertices' answer: the polyline to select.
    selection: Option<Vec<EntityId>>,
    // Done with nothing to do (a height set to what it is): no undo step.
    no_change: Option<String>,
    message: String,
    feedback: ToolFeedback,
}

type Planner = fn(&Collected, &ToolContext<'_>) -> Plan;
// What a step asks, given the one polyline in play, if any.
type PromptFn = Box<dyn Fn(&Collected, Option<EntityId>) -> String + Send + Sync>;
// Value: what Enter takes, shown as <default>.
type FallbackFn = Box<dyn Fn(&Collected) -> Option<String> + Send + Sync>;
// A pick refused whatever follows (Grade's vertex with no height).
type CheckFn = Box<dyn Fn(&Chosen) -> Option<String> + Send + Sync>;
// The label of the Target the pick shows before the tool can plan.
type LabelFn = Box<dyn Fn(&Chosen) -> String + Send + Sync>;
// Selection tools: the finish, as it always was.
type FinishFn = Box<dyn Fn(&Collected) -> ToolStep + Send + Sync>;
// What the tool's handles (and the selection it started on) answer: fills
// the collected picks and says how many leading steps that answers (0: none,
// the tool asks as usual).
type AnswerFn = fn(&ToolContext<'_>, &ToolStart, &mut Collected) -> usize;
// The prompt when the handles have answered every step.
type ReadyFn = fn(&Collected, Option<&Document>) -> String;
// Set Height: a click at the value step takes the height there.
type ValueFromClickFn = fn(&Collected, &ToolContext<'_>, Point2) -> Option<String>;
// What an edit made is remembered for the next use.
type RememberFn = fn(&Collected);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StepKind {
    // One vertex of a polyline, within the vertex reach.
    Vertex,
    // One segment of a polyline, within the pick reach.
    Segment,
    // One polyline, within the pick reach.
    Polyline,
    // A point ON a polyline (snapped, then put on it): where a vertex goes.
    OnLine,
    // A point (snapped): the vertex's new place, the arc's point.
    Point,
    // Typed text; Enter takes the default when there is one.
    Value,
    // Several polylines, then Enter.
    Selection,
}

struct Step {
    kind: StepKind,
    prompt: PromptFn,
    fallback: Option<FallbackFn>,
    // Vertex: on the polyline the pick before chose, as it was then.
    same_polyline: bool,
    // Vertex: [Other side] is offered when the polyline is closed.
    other_side: bool,
    // OnLine, Point: the point's height is taken - the z of x,y,z, the dz of
    // @dx,dy,dz (InteractiveTool::takes_heights) - by Insert's new vertex and
    // Move's new place; Segment to Arc's point has no use for one.
    heights: bool,
    check: Option<CheckFn>,
    label: Option<LabelFn>,
}

impl Step {
    fn new(kind: StepKind, prompt: PromptFn) -> Self {
        Self {
            kind,
            prompt,
            fallback: None,
            same_polyline: false,
            other_side: false,
            heights: false,
            check: None,
            label: None,
        }
    }
}

fn is_pick(kind: StepKind) -> bool {
    matches!(
        kind,
        StepKind::Vertex | StepKind::Segment | StepKind::Polyline | StepKind::OnLine
    )
}

#[derive(Default)]
struct Script {
    steps: Vec<Step>,
    // One-polyline tools: the plan, drawn and committed.
    plan: Option<Planner>,
    finish: Option<FinishFn>,
    answer: Option<AnswerFn>,
    // "Press Enter to delete vertex 2 of polyline 12, or click another
    // vertex".
    ready: Option<ReadyFn>,
    value_from_click: Option<ValueFromClickFn>,
    // Fillet's radius, Chamfer's distances: called once the plan is
    // committed, never by a preview, which plans at the remembered values.
    remember: Option<RememberFn>,
    restarts: bool,
    // Delete Vertex: the Delete key is its Enter (ScriptTool::delete_key).
    delete_is_enter: bool,
    // The tool's title, for what it says of itself ("Insert Vertex").
    title: String,
}

// A second pick of the vertex the pick before took (Straighten's, Grade's):
// refused, and the preview says so (R3) - it showed the first pick as taken,
// and a click the preview did not refuse was refused. Straight after that
// pick, with the cursor still on it, the view holds the refusal off
// (ToolHost::feedback), so the pick is not answered in red.
fn already_picked(c: &Collected, step: &Step, chosen: &Chosen) -> Option<String> {
    let last = c.picks.last()?;
    let vertex = chosen.vertex?;
    if !step.same_polyline || last.polyline != chosen.polyline || last.vertex != chosen.vertex {
        return None;
    }
    Some(format!(
        "vertex {vertex} is picked already; click the other one"
    ))
}

fn stale_sentence(id: EntityId) -> String {
    format!("polyline {id} changed since it was picked; pick it again")
}

fn polyline_now(document: Option<&Document>, id: EntityId) -> Option<CurvePolyline2> {
    read_polyline(document?.model().entities.find(id)?)
}

// Why `chosen` - picked by a click earlier in the tool, or chosen before it
// (a handle, Insert's anchor) - cannot be used as it was chosen: its polyline
// was edited since (the Vertices panel, an agent, another view), deleted, or
// its layer hidden or locked (THE visibility rule, pickable). None while it
// stands. Only fresh picks were checked: a kept one was drawn and planned
// from its copy, green, and the click was then refused as stale - or, on a
// layer hidden since, Enter edited what the view no longer drew.
fn why_not_now(cx: &ToolContext<'_>, chosen: &Chosen) -> Option<String> {
    let name = format!("polyline {}", chosen.polyline);
    let deleted = || format!("{name} was deleted since it was picked; pick another");
    let Some(document) = cx.document else {
        return Some(deleted());
    };
    let Some(entity) = document.model().entities.find(chosen.polyline) else {
        return Some(deleted());
    };
    if !pickable(cx, entity) {
        if is_drawn(document.model(), entity, view_of(cx)) {
            return Some(format!(
                "{name} is on the locked layer {}; unlock it to edit it",
                entity.layer
            ));
        }
        return Some(format!(
            "{name} is on a layer hidden since it was picked; show it to edit it"
        ));
    }
    if read_polyline(entity).as_ref() != Some(&chosen.shape) {
        return Some(stale_sentence(chosen.polyline));
    }
    None
}

fn polylines_among(document: Option<&Document>, ids: &[EntityId]) -> Vec<EntityId> {
    let Some(document) = document else {
        return Vec::new();
    };
    ids.iter()
        .copied()
        .filter(|&id| {
            document
                .model()
                .entities
                .find(id)
                .is_some_and(is_polyline_entity)
        })
        .collect()
}

// ONE command for a plan's edits, each checked against the shape it was
// planned from: a polyline changed since (by the panel, an agent, another
// view) is refused, never overwritten with the old shape.
fn commit_edits(name: String, edits: Vec<Edit>) -> CommandPtr {
    let mut ids = Vec::new();
    let mut by_id: BTreeMap<EntityId, Edit> = BTreeMap::new();
    for edit in edits {
        ids.push(edit.polyline);
        by_id.entry(edit.polyline).or_insert(edit);
    }
    edit_each_polyline(
        ids,
        name,
        move |_model: &Model, id: EntityId, now: &CurvePolyline2| -> PolylineResult {
            match by_id.get(&id) {
                Some(edit) if *now == edit.before => Ok(edit.after.clone()),
                _ => Err(Error::new(ErrorCode::InvalidState, stale_sentence(id))),
            }
        },
    )
}

// What a plan shows, its refusal or warning said.
fn feedback_of(plan: &Plan) -> ToolFeedback {
    let mut feedback = plan.feedback.clone();
    if let Some(refusal) = &plan.refusal {
        feedback.refused = true;
        feedback.caption = refusal.clone();
    } else if let Some(warning) = &plan.warning {
        feedback.caption = warning.clone();
    }
    feedback
}

// What a pick shows before the tool can plan: only what it takes.
fn target_of(step: &Step, chosen: &Chosen) -> ToolFeedback {
    let mut feedback = ToolFeedback {
        focus: Some(chosen.polyline),
        ..ToolFeedback::default()
    };
    if let Some(vertex) = chosen.vertex {
        let label = match &step.label {
            Some(label) => label(chosen),
            None => format!("{vertex}"),
        };
        feedback.marks.push(vertex_mark(
            FeedbackRole::Target,
            chosen.shape.vertices[vertex].position,
            label,
        ));
    } else if let Some(segment) = chosen.segment
        && step.kind == StepKind::Segment
    {
        feedback.marks.push(piece_mark(
            FeedbackRole::Target,
            piece_of(&chosen.shape, segment),
            "",
        ));
    } else {
        feedback.marks.push(piece_mark(
            FeedbackRole::Target,
            whole_of(&chosen.shape),
            "",
        ));
    }
    feedback
}

// `shape` (polyline `id`) as a candidate for `step` at `at`, with its
// distance; None when nothing of it is within `reach`.
fn candidate(
    step: &Step,
    id: EntityId,
    shape: &CurvePolyline2,
    at: Point2,
    reach: f64,
) -> Option<(Chosen, f64)> {
    let mut chosen = Chosen {
        polyline: id,
        shape: shape.clone(),
        ..Chosen::default()
    };
    let distance = match step.kind {
        StepKind::Vertex => {
            let vertex = nearest_vertex(shape, at)?;
            chosen.vertex = Some(vertex);
            chosen.at = shape.vertices[vertex].position;
            chosen.at.distance_to(at)
        }
        StepKind::Segment => {
            let segment = nearest_segment(shape, at)?;
            chosen.segment = Some(segment);
            chosen.at = closest_on_piece(shape, segment, at);
            distance_to_piece(shape, segment, at)
        }
        StepKind::Polyline | StepKind::OnLine => {
            let nearest = shape.nearest(at)?;
            if shape.segment_count() == 0 {
                return None;
            }
            chosen.segment = Some(nearest.segment);
            chosen.at = nearest.point;
            nearest.distance
        }
        StepKind::Point | StepKind::Value | StepKind::Selection => return None,
    };
    if !(distance <= reach) {
        return None;
    }
    Some((chosen, distance))
}

// ---- the state machine ---------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AnsweredBy {
    None,
    Selection,
    Handles,
}

struct ScriptTool {
    script: Arc<Script>,
    // How many entities were selected when the tool started: all stay_on
    // (R5) asks of the C++ context's selection.
    selection_size: usize,
    // The polylines selected when the tool started.
    own: Vec<EntityId>,
    step: usize,
    // The leading steps the selection or the handles answered.
    answered: usize,
    answered_by: AnsweredBy,
    stale: bool,
    collected: Collected,
}

impl ScriptTool {
    fn new(cx: &ToolContext<'_>, start: &ToolStart, script: Arc<Script>) -> Self {
        let mut tool = Self {
            script: Arc::clone(&script),
            selection_size: start.selection.len(),
            own: polylines_among(cx.document, &start.selection),
            step: 0,
            answered: 0,
            answered_by: AnsweredBy::None,
            stale: false,
            collected: Collected::default(),
        };
        let steps = &script.steps;
        if steps
            .first()
            .is_some_and(|step| step.kind == StepKind::Selection)
        {
            // A selection step is answered by the selection the tool started
            // on, when it holds polylines the view lets be edited (R1): a
            // selected polyline whose layer was then hidden was weeded,
            // closed or opened where nobody could see it.
            let usable = tool.pickable_among(cx, &tool.own);
            if !usable.is_empty() {
                tool.collected.selection = usable;
                tool.step = 1;
                tool.answered = 1;
                tool.answered_by = AnsweredBy::Selection;
            } else {
                let why = tool.locked_note(cx);
                if !why.is_empty() {
                    tool.collected.note = format!(
                        "{why}. {}",
                        (steps[0].prompt)(&tool.collected, tool.in_play(cx))
                    );
                }
            }
            return tool;
        }
        let mut why = String::new();
        if let Some(answer) = script.answer
            && cx.document.is_some()
        {
            let mut answered = tool.collected.clone();
            let count = answer(cx, start, &mut answered);
            let refused = if count > 0 {
                tool.refusal_of(cx, &answered, count)
            } else {
                None
            };
            if count > 0 && refused.is_none() {
                tool.collected = answered;
                tool.step = count;
                tool.answered = count;
                tool.answered_by = AnsweredBy::Handles;
                return tool;
            }
            tool.collected.anchor = answered.anchor.take();
            why = refused.unwrap_or(answered.note);
        }
        if why.is_empty() {
            why = tool.locked_note(cx);
        }
        if !why.is_empty()
            && let Some(first) = steps.first()
        {
            // What was chosen before the tool and cannot be used is never
            // dropped without a word: the reason, then the prompt.
            tool.collected.note = format!(
                "{why}. {}",
                (first.prompt)(&tool.collected, tool.in_play(cx))
            );
        }
        tool
    }

    // The one polyline known without a pick: the last pick's, the anchor's,
    // or the one selected polyline, when a pick may take it.
    fn in_play(&self, cx: &ToolContext<'_>) -> Option<EntityId> {
        if let Some(last) = self.collected.picks.last() {
            return Some(last.polyline);
        }
        if let Some(anchor) = &self.collected.anchor {
            return Some(anchor.polyline);
        }
        if self.own.len() == 1 && self.pickable_id(cx, self.own[0]) {
            return Some(self.own[0]);
        }
        None
    }

    fn pickable_id(&self, cx: &ToolContext<'_>, id: EntityId) -> bool {
        cx.document
            .and_then(|document| document.model().entities.find(id))
            .is_some_and(|entity| pickable(cx, entity))
    }

    // The polylines among `ids` a tool may take (pickable), in their order.
    fn pickable_among(&self, cx: &ToolContext<'_>, ids: &[EntityId]) -> Vec<EntityId> {
        ids.iter()
            .copied()
            .filter(|&id| self.pickable_id(cx, id))
            .collect()
    }

    // Why what the tool holds from before this input - its picks, Insert's
    // anchor - cannot be used (why_not_now), or None when all of it stands.
    fn staleness(&self, cx: &ToolContext<'_>) -> Option<String> {
        for pick in &self.collected.picks {
            if let Some(why) = why_not_now(cx, pick) {
                return Some(why);
            }
        }
        if let Some(anchor) = &self.collected.anchor {
            return why_not_now(cx, anchor);
        }
        None
    }

    // An input when what the tool holds is stale is refused with the reason,
    // as the preview said (R3), and the stale picks are dropped, so the next
    // input picks afresh: the tool starts again from its first step, as a
    // stale edit refused at the commit always has.
    fn refuse_stale(&mut self, cx: &ToolContext<'_>) -> Option<ToolStep> {
        let why = self.staleness(cx)?;
        self.drop_answers();
        Some(ToolStep::rejected(why))
    }

    // Beside a click's preview with the handles answering every step: what
    // Enter would act on, marked as Enter's with its number - with the
    // pointer over another vertex the chosen one was a plain square, and the
    // prompt's "vertex 2" was nowhere on screen. Not where the click's own
    // marks are: there the click and Enter do the same.
    fn mark_what_enter_takes(&self, click: &mut ToolFeedback) {
        let marks = &click.marks;
        let marked = |at: Point2| {
            marks.iter().any(|mark| {
                matches!(&mark.geometry, Geometry::Point(point)
                    if point.position.distance_to(at) <= tolerance::GEOMETRIC)
            })
        };
        let mut enter = Vec::new();
        for pick in &self.collected.picks {
            let (at, label) = if let Some(vertex) = pick.vertex {
                (pick.shape.vertices[vertex].position, format!("{vertex}"))
            } else if let Some(segment) = pick.segment {
                (
                    middle_of_piece(&pick.shape, segment),
                    format!("segment {segment}"),
                )
            } else {
                // A whole polyline (Edit Vertices'): no vertex of it is what
                // Enter takes, and one marked - its vertex 0, "polyline 2 ·
                // Enter" - read as a chosen vertex. The prompt names it.
                continue;
            };
            if !marked(at) {
                enter.push(vertex_mark(
                    FeedbackRole::Enter,
                    at,
                    format!("{label} \u{b7} Enter"),
                ));
            }
        }
        click.marks.extend(enter);
    }

    // Insert beside a chosen vertex or segment where a click takes nothing -
    // past the segment's ends, or on the chosen vertex itself
    // (beside_anchor) - shows what Enter does: the chosen vertex, the place
    // Enter adds at, and the caption said as Enter's. It showed "too close to
    // vertex N" in red, at any distance, the moment the tool started.
    fn beside_anchor_idle(&self, cx: &ToolContext<'_>, plan: Planner) -> ToolFeedback {
        let Some(anchor) = &self.collected.anchor else {
            return ToolFeedback::default();
        };
        let mut shown = ToolFeedback {
            focus: Some(anchor.polyline),
            ..ToolFeedback::default()
        };
        if let Some(vertex) = anchor.vertex {
            shown.marks.push(vertex_mark(
                FeedbackRole::Target,
                anchor.shape.vertices[vertex].position,
                format!("{vertex}"),
            ));
        }
        let middle = middle_beside(anchor);
        let place = middle.at;
        let mut c = self.collected.clone();
        c.picks.push(middle);
        let planned = plan(&self.with_defaults(c, self.step + 1), cx);
        let mut enter = vertex_mark(FeedbackRole::Enter, place, "Enter");
        enter.refused = planned.refusal.is_some();
        shown.marks.push(enter);
        shown.caption = format!(
            "Enter: {}",
            planned
                .refusal
                .as_deref()
                .unwrap_or(planned.feedback.caption.as_str())
        );
        shown
    }

    // Why a selected polyline will not be taken: it is drawn, but on a locked
    // layer (THE rule, pickable). A hidden one says nothing - it is not there
    // to be clicked. Empty when there is none.
    fn locked_note(&self, cx: &ToolContext<'_>) -> String {
        let Some(document) = cx.document else {
            return String::new();
        };
        for &id in &self.own {
            if let Some(entity) = document.model().entities.find(id)
                && !pickable(cx, entity)
                && is_drawn(document.model(), entity, view_of(cx))
            {
                return format!(
                    "polyline {id} is on the locked layer {}; unlock it to edit it",
                    entity.layer
                );
            }
        }
        String::new()
    }

    // What the preview shows where the cursor finds nothing new: the vertices
    // of the polyline in play, and what the picks before took - Straighten's
    // first "keep" stays marked while the cursor is off the line.
    fn idle(&self, cx: &ToolContext<'_>) -> ToolFeedback {
        let mut feedback = ToolFeedback {
            focus: self.in_play(cx),
            ..ToolFeedback::default()
        };
        let steps = &self.script.steps;
        let mut step = 0;
        for pick in &self.collected.picks {
            while step + 1 < steps.len() && !is_pick(steps[step].kind) {
                step += 1;
            }
            let taken = target_of(&steps[step], pick);
            feedback.marks.extend(taken.marks);
            if step + 1 < steps.len() {
                step += 1;
            }
        }
        feedback
    }

    // The pick a click at `cursor` would make for step `at` after `c`
    // collected, and the plan it leads to; None when nothing is in reach.
    fn pick_preview(
        &self,
        cx: &ToolContext<'_>,
        mut c: Collected,
        at: usize,
        cursor: Point2,
    ) -> Option<ToolFeedback> {
        let step = &self.script.steps[at];
        let chosen = self.resolve(cx, &c, step, cursor, false)?;
        if let Some(already) = already_picked(&c, step, &chosen) {
            // The first pick's marks, drawn as the refusal they now are.
            let mut taken = self.idle(cx);
            taken.refused = true;
            taken.caption = already;
            return Some(taken);
        }
        if let Some(check) = &step.check
            && let Some(why) = check(&chosen)
        {
            let mut refused = target_of(step, &chosen);
            refused.refused = true;
            refused.caption = why;
            return Some(refused);
        }
        c.picks.push(chosen);
        if !self.can_plan(&c, at + 1) {
            return c.picks.last().map(|last| target_of(step, last));
        }
        let plan = self.script.plan?;
        Some(feedback_of(&plan(&self.with_defaults(c, at + 1), cx)))
    }

    fn side_offered(&self) -> bool {
        let offers = self.script.steps.iter().any(|step| step.other_side);
        offers
            && self
                .collected
                .picks
                .first()
                .is_some_and(|first| first.shape.closed)
    }

    // Every step from `from` on is a value with a default: the tool can plan
    // (and so preview, and refuse) now, at those defaults.
    fn can_plan(&self, c: &Collected, from: usize) -> bool {
        self.script.steps.iter().skip(from).all(|step| {
            step.kind == StepKind::Value
                && step
                    .fallback
                    .as_ref()
                    .is_some_and(|fallback| fallback(c).is_some())
        })
    }

    fn with_defaults(&self, mut c: Collected, from: usize) -> Collected {
        for step in self.script.steps.iter().skip(from) {
            if let Some(fallback) = &step.fallback
                && let Some(value) = fallback(&c)
            {
                c.values.push(value);
            }
        }
        c
    }

    // Why what the handles answered cannot be taken - a pick its step's check
    // refuses, or the plan's refusal where the plan can be made - or None
    // when it can be.
    fn refusal_of(&self, cx: &ToolContext<'_>, c: &Collected, count: usize) -> Option<String> {
        for (i, step) in self.script.steps.iter().enumerate().take(count) {
            if let Some(check) = &step.check
                && let Some(pick) = c.picks.get(i)
                && let Some(why) = check(pick)
            {
                return Some(why);
            }
        }
        if let Some(plan) = self.script.plan
            && self.can_plan(c, count)
        {
            return plan(&self.with_defaults(c.clone(), count), cx).refusal;
        }
        None
    }

    // ---- what a pick takes (docs/drawing.md, R1 and R2) ----

    // What a pick at `at` takes for `step` after `c` collected: the anchor's
    // segment (Insert beside a hot vertex), else the pick before's polyline
    // (Straighten's second vertex), else the nearest selected polyline within
    // reach, else the polyline the view would pick there. Nothing else: never
    // another kind of entity, never a vertex out of reach, and never what the
    // view may not pick - a selected polyline on a hidden or locked layer was
    // previewed and edited through the selection, round the view's own pick.
    // `exact` for a point typed rather than pointed at (Chosen::exact).
    fn resolve(
        &self,
        cx: &ToolContext<'_>,
        c: &Collected,
        step: &Step,
        at: Point2,
        exact: bool,
    ) -> Option<Chosen> {
        let mut chosen = self.nearest_for(cx, c, step, at, exact)?;
        chosen.exact = exact;
        Some(chosen)
    }

    // resolve's search, before it marks what was found as given exactly or
    // pointed at; `exact` also takes the pixel reach off Insert's anchor.
    fn nearest_for(
        &self,
        cx: &ToolContext<'_>,
        c: &Collected,
        step: &Step,
        at: Point2,
        exact: bool,
    ) -> Option<Chosen> {
        if step.kind == StepKind::OnLine
            && let Some(anchor) = &c.anchor
        {
            return beside_anchor(anchor, at, if exact { 0.0 } else { pick_reach(cx) });
        }
        let reach = if step.kind == StepKind::Vertex {
            vertex_reach(cx)
        } else {
            pick_reach(cx)
        };
        if step.same_polyline
            && let Some(first) = c.picks.last()
        {
            return candidate(step, first.polyline, &first.shape, at, reach)
                .map(|(chosen, _)| chosen);
        }
        let document = cx.document?;
        let mut best: Option<(Chosen, f64)> = None;
        if !self.own.is_empty() {
            let mut nearby = Vec::new();
            document
                .spatial_index()
                .query_radius(at, reach, &mut nearby);
            for id in nearby {
                if self.own.binary_search(&id).is_err() || !self.pickable_id(cx, id) {
                    continue;
                }
                let Some(shape) = polyline_now(Some(document), id) else {
                    continue;
                };
                if let Some(found) = candidate(step, id, &shape, at, reach)
                    && best.as_ref().is_none_or(|best| found.1 < best.1)
                {
                    best = Some(found);
                }
            }
        }
        if let Some((chosen, _)) = best {
            return Some(chosen);
        }
        if let Some(id) = pick_under(cx, at, &polyline_kinds(), Some(reach))
            && let Some(shape) = polyline_now(Some(document), id)
            && let Some((chosen, _)) = candidate(step, id, &shape, at, reach)
        {
            return Some(chosen);
        }
        None
    }

    // ---- inputs ----

    // A click or a number after the handles answered every step picks anew,
    // from the first step. Refused, it leaves the handles as they were: a
    // Rejected step leaves the tool unchanged, and a click on nothing had
    // dropped them.
    fn anew(&mut self, pick: impl FnOnce(&mut Self) -> ToolStep) -> ToolStep {
        let saved = (
            self.step,
            self.answered,
            self.answered_by,
            self.collected.clone(),
        );
        self.drop_answers();
        let step = pick(self);
        if step.outcome == ToolStepOutcome::Rejected {
            (self.step, self.answered, self.answered_by, self.collected) = saved;
        }
        step
    }

    // Why a TYPED point at a pick took nothing. A typed x,y is taken as a
    // click there is (R1), within the view's reach at this zoom - a vertex
    // within 12 px, a polyline within 8 - and one that missed was answered
    // with the prompt alone, though the typist cannot see the reach. Insert
    // beside a chosen vertex is not a reach but the anchor's segments (its
    // prompt says so), and says the prompt as before.
    fn missed(&self, cx: &ToolContext<'_>, step: &Step, at: Point2) -> String {
        if step.kind == StepKind::OnLine && self.collected.anchor.is_some() {
            return self.prompt(cx);
        }
        let vertex = step.kind == StepKind::Vertex;
        let reach = if vertex {
            vertex_reach(cx)
        } else {
            pick_reach(cx)
        };
        format!(
            "{},{} is not within {} of {} the tool can take: a typed point is taken as a click \
             there is, within the view's {} px at this zoom{}{}",
            number(at.x),
            number(at.y),
            fixed(reach, 3),
            if vertex { "a vertex" } else { "a polyline" },
            number(if vertex {
                SNAP_APERTURE_PIXELS
            } else {
                PICK_APERTURE_PIXELS
            }),
            if vertex {
                "; type the vertex's number, or zoom out. "
            } else {
                "; zoom out. "
            },
            self.prompt(cx)
        )
    }

    // A click (exact false) or a typed point (true) at `at`, with the height
    // typed for it, if any.
    fn point_at(
        &mut self,
        cx: &ToolContext<'_>,
        at: Point2,
        z: Option<f64>,
        exact: bool,
    ) -> ToolStep {
        if let Some(stale) = self.refuse_stale(cx) {
            return stale;
        }
        let script = Arc::clone(&self.script);
        let steps = &script.steps;
        if self.step >= steps.len() {
            if self.answered_by != AnsweredBy::Handles {
                return default_point(cx, self.expects(cx), at);
            }
            if self
                .resolve(cx, &Collected::default(), &steps[0], at, exact)
                .is_none()
            {
                return ToolStep::rejected(if exact {
                    self.missed(cx, &steps[0], at)
                } else {
                    self.prompt(cx)
                });
            }
            return self.anew(|tool| tool.point_at(cx, at, z, exact));
        }
        let step = &steps[self.step];
        match step.kind {
            StepKind::Vertex | StepKind::Segment | StepKind::Polyline | StepKind::OnLine => {
                let Some(chosen) = self.resolve(cx, &self.collected, step, at, exact) else {
                    return ToolStep::rejected(if exact {
                        self.missed(cx, step, at)
                    } else {
                        self.prompt(cx)
                    });
                };
                let height = if step.kind == StepKind::OnLine {
                    z
                } else {
                    None
                };
                self.take(cx, chosen, height)
            }
            StepKind::Point => {
                self.collected.points.push(at);
                self.collected.height = z;
                self.advance(cx)
            }
            StepKind::Value => {
                if let Some(value_from_click) = script.value_from_click {
                    let Some(taken) = value_from_click(&self.collected, cx, at) else {
                        return ToolStep::rejected(format!(
                            "nothing with a height is within reach there; {}",
                            self.prompt(cx)
                        ));
                    };
                    self.collected.values.push(taken);
                    return self.advance(cx);
                }
                ToolStep::rejected(format!("type a value: {}", self.prompt(cx)))
            }
            StepKind::Selection => default_point(cx, self.expects(cx), at),
        }
    }

    // A typed whole number at a pick: vertex (or segment) N of the polyline
    // in play - a headless run can drive every pick tool by typing.
    fn pick_by_number(&mut self, cx: &ToolContext<'_>, index: usize) -> ToolStep {
        let script = Arc::clone(&self.script);
        if self.step >= script.steps.len() {
            return self.anew(|tool| tool.pick_by_number(cx, index));
        }
        let step = &script.steps[self.step];
        let (id, shape) = if step.same_polyline
            && let Some(last) = self.collected.picks.last()
        {
            (last.polyline, Some(last.shape.clone()))
        } else if let Some(anchor) = &self.collected.anchor {
            (anchor.polyline, Some(anchor.shape.clone()))
        } else if self.own.len() == 1 && self.pickable_id(cx, self.own[0]) {
            (self.own[0], polyline_now(cx.document, self.own[0]))
        } else {
            (INVALID_ENTITY_ID, None)
        };
        let Some(shape) = shape else {
            return ToolStep::rejected(
                "a number names a vertex of the one selected polyline; select one polyline, or \
                 click instead",
            );
        };
        let mut chosen = Chosen {
            polyline: id,
            shape,
            // Named, not pointed at.
            exact: true,
            ..Chosen::default()
        };
        match step.kind {
            StepKind::Vertex => {
                if index >= chosen.shape.vertices.len() {
                    return ToolStep::rejected(format!(
                        "polyline {id} has no vertex {index}; its vertices are 0 to {}",
                        chosen.shape.vertices.len().wrapping_sub(1)
                    ));
                }
                chosen.vertex = Some(index);
                chosen.at = chosen.shape.vertices[index].position;
            }
            StepKind::Segment | StepKind::OnLine => {
                if index >= chosen.shape.segment_count() {
                    return ToolStep::rejected(format!("polyline {id} has no segment {index}"));
                }
                chosen.segment = Some(index);
                chosen.at = middle_of_piece(&chosen.shape, index);
            }
            StepKind::Polyline | StepKind::Point | StepKind::Value | StepKind::Selection => {
                return ToolStep::rejected(format!(
                    "'{index}' is not taken here; {}",
                    self.prompt(cx)
                ));
            }
        }
        self.take(cx, chosen, None)
    }

    // A pick, taken unless its step or the plan refuses it (R3): the refusal
    // comes at the pick, before any value it would be followed by.
    fn take(&mut self, cx: &ToolContext<'_>, chosen: Chosen, z: Option<f64>) -> ToolStep {
        let script = Arc::clone(&self.script);
        let step = &script.steps[self.step];
        if let Some(already) = already_picked(&self.collected, step, &chosen) {
            return ToolStep::rejected(already);
        }
        if let Some(check) = &step.check
            && let Some(why) = check(&chosen)
        {
            return ToolStep::rejected(why);
        }
        let mut next = self.collected.clone();
        next.picks.push(chosen);
        if let Some(z) = z {
            next.height = Some(z);
        }
        if let Some(plan) = script.plan
            && self.can_plan(&next, self.step + 1)
            && let Some(refusal) =
                plan(&self.with_defaults(next.clone(), self.step + 1), cx).refusal
        {
            return ToolStep::rejected(refusal);
        }
        self.collected = next;
        self.advance(cx)
    }

    fn advance(&mut self, cx: &ToolContext<'_>) -> ToolStep {
        self.step += 1;
        if self.step < self.script.steps.len() {
            return ToolStep::next("");
        }
        self.finish(cx)
    }

    fn finish(&mut self, cx: &ToolContext<'_>) -> ToolStep {
        self.stale = false;
        let script = Arc::clone(&self.script);
        let mut done = if script.plan.is_some() {
            self.commit(cx)
        } else if let Some(finish) = &script.finish {
            finish(&self.collected)
        } else {
            ToolStep::done(None, "", false)
        };
        if done.outcome == ToolStepOutcome::Rejected {
            if self.stale {
                // The pick is out of date: start again from it.
                self.drop_answers();
            } else if self.step > self.answered {
                // The last input was not acceptable (a radius that does not
                // fit): step back to it, keeping everything before.
                self.step_back();
            }
            return done;
        }
        done.restart = script.restarts;
        self.step = 0;
        self.answered = 0;
        self.answered_by = AnsweredBy::None;
        self.collected = Collected::default();
        done
    }

    fn commit(&mut self, cx: &ToolContext<'_>) -> ToolStep {
        let Some(plan_of) = self.script.plan else {
            return ToolStep::done(None, "", false);
        };
        let plan = plan_of(&self.collected, cx);
        if let Some(refusal) = &plan.refusal {
            return ToolStep::rejected(refusal.clone());
        }
        if let Some(warning) = &plan.warning {
            return ToolStep::rejected(warning.clone());
        }
        if let Some(no_change) = &plan.no_change {
            return ToolStep::done(None, no_change.clone(), false);
        }
        for edit in &plan.edits {
            let now = polyline_now(cx.document, edit.polyline);
            if now.as_ref() != Some(&edit.before) {
                self.stale = true;
                return ToolStep::rejected(stale_sentence(edit.polyline));
            }
        }
        if plan.edits.is_empty() {
            let mut step = ToolStep::done(None, plan.message, false);
            step.selection = plan.selection;
            return step;
        }
        if let Some(remember) = self.script.remember {
            remember(&self.collected);
        }
        let selection = match &plan.selection {
            Some(selection) => Some(selection.clone()),
            None => self.stay_on(&plan),
        };
        let Plan {
            command,
            edits,
            message,
            ..
        } = plan;
        let mut step = ToolStep::done(Some(commit_edits(command, edits)), message, false);
        step.selection = selection;
        step
    }

    // R5: after an edit the tool stays on the polyline - it becomes the
    // selection, so the restarted tool has it as its own and the next edit is
    // one click - unless the user had selected several things. One thing
    // selected moves to the polyline just edited: that one was mostly the
    // tool's own choice after an earlier edit, and kept, it had the prompt
    // naming the first polyline while the user worked on another.
    fn stay_on(&self, plan: &Plan) -> Option<Vec<EntityId>> {
        if !self.script.restarts || plan.edits.len() != 1 {
            return None;
        }
        let id = plan.edits[0].polyline;
        if self.selection_size <= 1 {
            return Some(vec![id]);
        }
        None
    }

    fn step_back(&mut self) {
        if self.step == 0 {
            return;
        }
        self.step -= 1;
        match self.script.steps[self.step].kind {
            StepKind::Vertex | StepKind::Segment | StepKind::Polyline | StepKind::OnLine => {
                self.collected.picks.pop();
                self.collected.height = None;
            }
            StepKind::Point => {
                self.collected.points.pop();
                self.collected.height = None;
            }
            StepKind::Value => {
                self.collected.values.pop();
            }
            StepKind::Selection => {}
        }
    }

    // Back to the first step with nothing collected: the handles (and the
    // note about them) are the user's choice before the tool, and a new pick
    // replaces them.
    fn drop_answers(&mut self) {
        self.step = 0;
        self.answered = 0;
        self.answered_by = AnsweredBy::None;
        self.collected = Collected::default();
    }
}

impl InteractiveTool for ScriptTool {
    fn prompt(&self, cx: &ToolContext<'_>) -> String {
        let steps = &self.script.steps;
        if self.step >= steps.len() {
            if self.answered_by == AnsweredBy::Handles
                && let Some(ready) = self.script.ready
            {
                return ready(&self.collected, cx.document);
            }
            return format!(
                "Press Enter to apply to the {}",
                plural(self.collected.selection.len(), "selected polyline")
            );
        }
        let step = &steps[self.step];
        let mut text = if self.step == 0 && !self.collected.note.is_empty() {
            self.collected.note.clone()
        } else {
            (step.prompt)(&self.collected, self.in_play(cx))
        };
        if let Some(fallback) = &step.fallback
            && let Some(shown) = fallback(&self.collected)
        {
            text.push_str(&format!(" <{shown}>"));
        }
        if step.other_side && self.side_offered() {
            text.push_str(" [Other side]");
        }
        text
    }

    fn expects(&self, _cx: &ToolContext<'_>) -> ToolInput {
        let steps = &self.script.steps;
        if self.step >= steps.len() {
            // Answered by handles, a click picks anew: an entity pick.
            return if self.answered_by == AnsweredBy::Handles {
                ToolInput::Entity
            } else {
                ToolInput::Value
            };
        }
        match steps[self.step].kind {
            StepKind::Vertex | StepKind::Segment | StepKind::Polyline => ToolInput::Entity,
            StepKind::OnLine | StepKind::Point => ToolInput::Point,
            StepKind::Value => ToolInput::Value,
            StepKind::Selection => ToolInput::Selection,
        }
    }

    // The view picks any kind of entity at its own aperture; what a pick
    // takes is resolve()'s, so the preview and the click cannot disagree.
    fn entity(&mut self, cx: &ToolContext<'_>, _id: EntityId, at: Point2) -> ToolStep {
        self.point_at(cx, at, None, false)
    }

    fn point(&mut self, cx: &ToolContext<'_>, at: Point2) -> ToolStep {
        self.point_at(cx, at, None, false)
    }

    // Every typed point, with a height or without, comes here - so point3d,
    // which only the default exact_point calls, is not overridden.
    fn exact_point(&mut self, cx: &ToolContext<'_>, at: Point2, z: Option<f64>) -> ToolStep {
        self.point_at(cx, at, z, true)
    }

    // A height is taken with a point where the step uses one (Step::heights):
    // Insert's new vertex and Move's new place. At a pick, or at Segment to
    // Arc's point, a height means nothing, and a typed dz there was refused
    // for want of one to change.
    fn takes_heights(&self, _cx: &ToolContext<'_>) -> bool {
        self.script
            .steps
            .get(self.step)
            .is_some_and(|step| step.heights)
    }

    fn value(&mut self, cx: &ToolContext<'_>, text: &str) -> ToolStep {
        if let Some(stale) = self.refuse_stale(cx) {
            return stale;
        }
        let typed = trimmed(text);
        let script = Arc::clone(&self.script);
        let steps = &script.steps;
        let at_value = self.step < steps.len() && steps[self.step].kind == StepKind::Value;
        let at_pick = if self.step < steps.len() {
            is_pick(steps[self.step].kind)
        } else {
            self.answered_by == AnsweredBy::Handles
        };
        if !at_value && self.side_offered() && is_option(typed, "Other") {
            self.collected.other_side = !self.collected.other_side;
            return ToolStep::next("the other side");
        }
        if at_value {
            self.collected.values.push(String::from(typed));
            return self.advance(cx);
        }
        if at_pick && let Some(index) = whole_number(typed) {
            return self.pick_by_number(cx, index);
        }
        ToolStep::rejected(format!("'{typed}' is not taken here; {}", self.prompt(cx)))
    }

    fn enter(&mut self, cx: &ToolContext<'_>) -> ToolStep {
        if let Some(stale) = self.refuse_stale(cx) {
            return stale;
        }
        let script = Arc::clone(&self.script);
        let steps = &script.steps;
        if self.step >= steps.len() {
            if self.answered_by != AnsweredBy::None {
                return self.finish(cx);
            }
            return ToolStep::done(None, "", false);
        }
        let step = &steps[self.step];
        if step.kind == StepKind::Value
            && let Some(fallback) = &step.fallback
            && let Some(value) = fallback(&self.collected)
        {
            self.collected.values.push(value);
            return self.advance(cx);
        }
        if step.kind == StepKind::Selection {
            let selected = match cx.document {
                Some(document) => polylines_among(Some(document), &document.selection().ids()),
                None => Vec::new(),
            };
            let polylines = self.pickable_among(cx, &selected);
            if polylines.is_empty() {
                return ToolStep::rejected(if selected.is_empty() {
                    "no polyline is selected; select polylines, then press Enter"
                } else {
                    "the selected polylines are on hidden or locked layers; select others, then \
                     press Enter"
                });
            }
            self.collected.selection = polylines;
            return self.advance(cx);
        }
        if step.kind == StepKind::OnLine
            && let Some(anchor) = &self.collected.anchor
        {
            let middle = middle_beside(anchor);
            return self.take(cx, middle, None);
        }
        if self.step == self.answered {
            // Nothing collected: Enter ends the tool.
            return ToolStep::done(None, "", false);
        }
        ToolStep::rejected(self.prompt(cx))
    }

    fn undo(&mut self, _cx: &ToolContext<'_>) -> ToolStep {
        if self.step <= self.answered {
            return ToolStep::rejected("nothing to undo in this tool");
        }
        self.step_back();
        ToolStep::next("")
    }

    fn preview(&self, cx: &ToolContext<'_>, cursor: Point2) -> ToolFeedback {
        let Some(plan) = self.script.plan else {
            return ToolFeedback::default();
        };
        if let Some(why) = self.staleness(cx) {
            // What the tool holds is out of date: no mark where the polyline
            // was, and the next input is refused with this and drops it.
            return ToolFeedback {
                refused: true,
                caption: why,
                ..ToolFeedback::default()
            };
        }
        let steps = &self.script.steps;
        if self.step >= steps.len() {
            // The handles answered every step, and a click picks anew (R3):
            // where one would take something the preview is THAT click's -
            // it drew the handles' plan, and a click deleted a vertex nobody
            // had marked - with what Enter would act on still marked beside
            // it. Elsewhere a click takes nothing, and the preview is what
            // Enter does, said as Enter's.
            if let Some(mut click) = self.pick_preview(cx, Collected::default(), 0, cursor) {
                self.mark_what_enter_takes(&mut click);
                return click;
            }
            let mut enter = feedback_of(&plan(&self.collected, cx));
            if !enter.caption.is_empty() {
                enter.caption = format!("Enter: {}", enter.caption);
            }
            return enter;
        }
        let step = &steps[self.step];
        if step.kind == StepKind::Selection {
            return ToolFeedback::default();
        }
        if is_pick(step.kind) {
            if let Some(shown) = self.pick_preview(cx, self.collected.clone(), self.step, cursor) {
                return shown;
            }
            if step.kind == StepKind::OnLine && self.collected.anchor.is_some() {
                return self.beside_anchor_idle(cx, plan);
            }
            return self.idle(cx);
        }
        if step.kind == StepKind::Value
            && let Some(value_from_click) = self.script.value_from_click
        {
            // A click here takes the height of what lies under it (Set
            // Height), so the preview is that click's plan where one is in
            // reach; elsewhere a click is refused, and the preview is what
            // Enter does, said as Enter's (R3). It showed the plan at the
            // default wherever the cursor was: neither the height a click
            // took nor the refusal of a click on nothing.
            if let Some(taken) = value_from_click(&self.collected, cx, cursor) {
                let mut c = self.collected.clone();
                c.values.push(taken);
                return feedback_of(&plan(&self.with_defaults(c, self.step + 1), cx));
            }
            if !self.can_plan(&self.collected, self.step) {
                return self.idle(cx);
            }
            let mut enter = feedback_of(&plan(
                &self.with_defaults(self.collected.clone(), self.step),
                cx,
            ));
            if !enter.caption.is_empty() {
                enter.caption = format!("Enter: {}", enter.caption);
            }
            return enter;
        }
        let mut c = self.collected.clone();
        let mut from = self.step;
        if step.kind == StepKind::Point {
            c.points.push(cursor);
            from += 1;
        }
        if !self.can_plan(&c, from) {
            return self.idle(cx);
        }
        feedback_of(&plan(&self.with_defaults(c, from), cx))
    }

    fn last_point(&self, _cx: &ToolContext<'_>) -> Option<Point2> {
        // Relative input (@dx,dy) is from the picked vertex: "move this
        // vertex 2 m east" is @2,0.
        let last = self.collected.picks.last()?;
        Some(match last.vertex {
            Some(vertex) => last.shape.vertices[vertex].position,
            None => last.at,
        })
    }

    // The picked vertex's height, which Move Vertex's @dx,dy,dz changes.
    fn last_height(&self, _cx: &ToolContext<'_>) -> Option<f64> {
        let last = self.collected.picks.last()?;
        last.shape.vertices[last.vertex?].height
    }

    // "Raise it by dz", as the VERTEX MOVE verb reads @dx,dy,dz.
    fn dz_is_a_change(&self, _cx: &ToolContext<'_>) -> bool {
        true
    }

    // The Delete key is the tool's while it runs, never the window's Erase of
    // the polyline it is editing (InteractiveTool::takes_delete). In Delete
    // Vertex with vertices chosen before it, Delete is its Enter: it deletes
    // them. Anywhere else it does nothing, and says so.
    fn takes_delete(&self, _cx: &ToolContext<'_>) -> bool {
        true
    }

    fn delete_key(&mut self, cx: &ToolContext<'_>) -> ToolStep {
        if self.script.delete_is_enter
            && self.step >= self.script.steps.len()
            && self.answered_by == AnsweredBy::Handles
        {
            return self.enter(cx);
        }
        if self.script.delete_is_enter {
            return ToolStep::rejected(format!(
                "Delete deletes the vertices chosen before {}, and none is; {}",
                self.script.title,
                self.prompt(cx)
            ));
        }
        ToolStep::rejected(format!(
            "Delete erases nothing while {} runs - it would take the polyline being edited; \
             press Esc first to erase the selection",
            self.script.title
        ))
    }

    // A point ON a polyline (Insert's) takes a snap only where it lands on
    // the line the pick takes there, within a pixel, and the click there
    // would be taken. So an arc segment's Center - offered while the arc is
    // hovered - another string's end or middle beside the line, and the
    // line's own vertices (always too close to insert at) are passed over,
    // and the cursor stays on the line; its Midpoint and a crossing are kept.
    fn takes_snap(&self, cx: &ToolContext<'_>, snap: &SnapResult) -> bool {
        let Some(step) = self
            .script
            .steps
            .get(self.step)
            .filter(|step| step.kind == StepKind::OnLine)
        else {
            return true;
        };
        let Some(chosen) = self.resolve(cx, &self.collected, step, snap.point, false) else {
            return false;
        };
        // One pixel: pick_reach is the view's pick aperture in model units.
        let pixel = pick_reach(cx) / PICK_APERTURE_PIXELS;
        if chosen.at.distance_to(snap.point) > pixel {
            return false;
        }
        let mut c = self.collected.clone();
        c.picks.push(chosen);
        if let Some(plan) = self.script.plan
            && self.can_plan(&c, self.step + 1)
        {
            return plan(&self.with_defaults(c, self.step + 1), cx)
                .refusal
                .is_none();
        }
        true
    }
}

// ---- handles: what was chosen before the tool ------------------------------------------------

// The hot grips of `kind` on polylines, as picks, each checked against the
// polyline as it is now: a grip whose vertex has moved since is not the
// vertex it names any more, and one whose layer has been hidden or locked
// since may not be taken (pickable); both are left out.
fn handles_of(cx: &ToolContext<'_>, handles: &[Grip], kind: GripKind) -> Vec<Chosen> {
    let mut out = Vec::new();
    for grip in handles {
        if grip.kind != kind {
            continue;
        }
        let Some(entity) = cx
            .document
            .and_then(|document| document.model().entities.find(grip.entity))
        else {
            continue;
        };
        if !pickable(cx, entity) {
            continue;
        }
        let Some(shape) = read_polyline(entity) else {
            continue;
        };
        let mut chosen = Chosen {
            polyline: grip.entity,
            shape,
            from_handle: true,
            ..Chosen::default()
        };
        if kind == GripKind::Vertex {
            if grip.index >= chosen.shape.vertices.len() {
                continue;
            }
            chosen.vertex = Some(grip.index);
            chosen.at = chosen.shape.vertices[grip.index].position;
        } else {
            if grip.index >= chosen.shape.segment_count() {
                continue;
            }
            chosen.segment = Some(grip.index);
            chosen.at = middle_of_piece(&chosen.shape, grip.index);
        }
        if chosen.at.distance_to(grip.position) > tolerance::GEOMETRIC {
            continue;
        }
        out.push(chosen);
    }
    out
}

// The segment between two hot vertices that are neighbours on one polyline.
fn segment_between(a: &Chosen, b: &Chosen) -> Option<Chosen> {
    if a.polyline != b.polyline {
        return None;
    }
    let (Some(first), Some(second)) = (a.vertex, b.vertex) else {
        return None;
    };
    let shape = &a.shape;
    let segment = if first < shape.segment_count() && shape.segment_end(first) == second {
        first
    } else if second < shape.segment_count() && shape.segment_end(second) == first {
        second
    } else {
        return None;
    };
    let mut chosen = a.clone();
    chosen.vertex = None;
    chosen.segment = Some(segment);
    chosen.at = middle_of_piece(shape, segment);
    Some(chosen)
}

// Why two hot vertices name no segment, for a tool that wanted one.
fn no_segment_between(a: &Chosen, b: &Chosen) -> String {
    if a.polyline != b.polyline {
        return String::from(
            "the two chosen vertices are on different polylines, so they name no segment",
        );
    }
    format!(
        "vertices {} and {} are not neighbours, so they name no segment",
        a.vertex.unwrap_or_default(),
        b.vertex.unwrap_or_default()
    )
}

fn one_vertex(cx: &ToolContext<'_>, start: &ToolStart, c: &mut Collected) -> usize {
    let hot = handles_of(cx, &start.handles, GripKind::Vertex);
    if hot.len() > 1 {
        c.note = format!("{} vertices are chosen, and this tool takes one", hot.len());
    }
    if hot.len() != 1 {
        return 0;
    }
    c.picks = hot;
    1
}

fn two_vertices(cx: &ToolContext<'_>, start: &ToolStart, c: &mut Collected) -> usize {
    let hot = handles_of(cx, &start.handles, GripKind::Vertex);
    if hot.len() == 2 && hot[0].polyline == hot[1].polyline && hot[0].vertex != hot[1].vertex {
        c.picks = hot;
        return 2;
    }
    if hot.len() == 1 {
        c.picks = hot;
        return 1;
    }
    if hot.len() == 2 {
        c.note = String::from(
            "the two chosen vertices are on different polylines, and this tool takes two on one",
        );
    } else if hot.len() > 2 {
        c.note = format!(
            "{} vertices are chosen, and this tool takes two on one polyline",
            hot.len()
        );
    }
    0
}

fn one_segment(cx: &ToolContext<'_>, start: &ToolStart, c: &mut Collected) -> usize {
    let segments = handles_of(cx, &start.handles, GripKind::SegmentMid);
    if segments.len() == 1 {
        c.picks = segments;
        return 1;
    }
    let hot = handles_of(cx, &start.handles, GripKind::Vertex);
    if segments.is_empty() && hot.len() == 2 {
        if let Some(between) = segment_between(&hot[0], &hot[1]) {
            c.picks = vec![between];
            return 1;
        }
        c.note = no_segment_between(&hot[0], &hot[1]);
    } else if segments.len() > 1 {
        c.note = format!(
            "{} segment middles are chosen, and this tool takes one",
            segments.len()
        );
    } else if !hot.is_empty() {
        c.note = String::from(
            "a segment is chosen by its middle grip, or by the two vertices at its ends",
        );
    }
    0
}

fn picked_vertex(c: &Collected, i: usize) -> String {
    format!(
        "vertex {} of polyline {}",
        c.picks[i].vertex.unwrap_or_default(),
        c.picks[i].polyline
    )
}

// " from vertex 1 to 3": Straighten's and Grade's two picks in the order the
// edit walks them - the side it takes, O's other side once typed
// (vertex_editing's vertex_range, as the plan). Said in the order picked,
// the prompt read the same before and after O while the caption and the
// marks changed side.
fn walked_between(c: &Collected) -> String {
    let (from, to) = vertex_range(
        &c.picks[0].shape,
        c.picks[0].vertex.unwrap_or_default(),
        c.picks[1].vertex.unwrap_or_default(),
        c.other_side,
    );
    format!(" from vertex {from} to {to}")
}

// ---- prompts ---------------------------------------------------------------------------------

// "Click the vertex to delete", or with a polyline in play "Click the vertex
// of polyline 12 to delete": never "select the polyline" when it is known.
fn click_the(thing: &str, purpose: &str) -> PromptFn {
    let thing = String::from(thing);
    let purpose = String::from(purpose);
    Box::new(move |_c: &Collected, in_play: Option<EntityId>| {
        let of = match in_play {
            Some(id) => format!(" of polyline {id}"),
            None => String::new(),
        };
        format!("Click the {thing}{of} {purpose}")
    })
}

fn fixed_prompt(text: &str) -> PromptFn {
    let text = String::from(text);
    Box::new(move |_c: &Collected, _in_play: Option<EntityId>| text.clone())
}

fn vertex_step(prompt: PromptFn) -> Step {
    Step::new(StepKind::Vertex, prompt)
}

fn segment_step(prompt: PromptFn) -> Step {
    Step::new(StepKind::Segment, prompt)
}

fn point_step(prompt: &str) -> Step {
    Step::new(StepKind::Point, fixed_prompt(prompt))
}

fn value_step(prompt: PromptFn, fallback: Option<FallbackFn>) -> Step {
    Step {
        fallback,
        ..Step::new(StepKind::Value, prompt)
    }
}

fn select_polylines(verb: &str) -> Step {
    Step::new(
        StepKind::Selection,
        fixed_prompt(&format!("Select polylines to {verb}, then press Enter")),
    )
}

// The value the tools remember, read when the prompt is shown.
fn remembered(read: fn(&VertexDefaults) -> f64) -> Option<FallbackFn> {
    Some(Box::new(move |_c: &Collected| {
        Some(number(read(&defaults())))
    }))
}

// ---- the plans --------------------------------------------------------------------------------

fn plan_insert(c: &Collected, cx: &ToolContext<'_>) -> Plan {
    let on = &c.picks[0];
    let shape = &on.shape;
    let s = on.segment.unwrap_or_default();
    let a = s;
    let b = shape.segment_end(s);
    let mut plan = Plan {
        command: String::from("VERTEX_INSERT"),
        ..Plan::default()
    };
    let f = &mut plan.feedback;
    f.focus = Some(on.polyline);
    // A POINTED click this near a vertex is taken as meant for the vertex: at
    // the view's pick aperture a new vertex there could not be told from it.
    // A place given exactly - Enter's marked middle, a typed number or point
    // - is refused only where geometry refuses it, on a vertex: the pixel
    // rule turned down the marked middle of every segment 16 px long or
    // less, where the prompt had just offered it.
    let to_a = on.at.distance_to(shape.vertices[a].position);
    let to_b = on.at.distance_to(shape.vertices[b].position);
    let too_near = if !on.exact && cpp::min(to_a, to_b) <= pick_reach(cx) {
        Some(if to_a <= to_b { a } else { b })
    } else {
        None
    };
    if let Some(anchor) = &c.anchor
        && let Some(vertex) = anchor.vertex
        && too_near != anchor.vertex
    {
        f.marks.push(vertex_mark(
            FeedbackRole::Target,
            shape.vertices[vertex].position,
            format!("{vertex}"),
        ));
    }
    if let Some(anchor) = &c.anchor {
        // Where Enter puts it - the middle of a segment the cursor may not be
        // on: it inserted on an edge the preview never marked. Not when the
        // pick IS that place (Enter's own plan). Struck through where Enter
        // would be refused: a segment with no length has its middle on a
        // vertex.
        let enter = middle_beside(anchor);
        if !(enter.segment == on.segment && enter.at.distance_to(on.at) <= tolerance::GEOMETRIC) {
            let mut place = vertex_mark(FeedbackRole::Enter, enter.at, "Enter");
            let e = enter.segment.unwrap_or_default();
            place.refused = enter.at.distance_to(shape.vertices[e].position)
                <= tolerance::GEOMETRIC
                || enter
                    .at
                    .distance_to(shape.vertices[shape.segment_end(e)].position)
                    <= tolerance::GEOMETRIC;
            f.marks.push(place);
        }
    }
    if let Some(near) = too_near {
        // The vertex it is too near, struck through with its number: the
        // whole segment in red flashed at every vertex the pointer passed,
        // and said nothing of which vertex was meant.
        let mut mark = vertex_mark(
            FeedbackRole::Target,
            shape.vertices[near].position,
            format!("{near}"),
        );
        mark.refused = true;
        f.marks.push(mark);
        plan.refusal = Some(format!(
            "too close to vertex {near}; zoom in to add a vertex this near it"
        ));
        return plan;
    }
    f.marks
        .push(piece_mark(FeedbackRole::Target, piece_of(shape, s), ""));
    let result = match insert_vertex(shape, s, on.at, c.height) {
        Ok(result) => result,
        Err(error) => {
            if let Some(last) = f.marks.last_mut() {
                last.refused = true;
            }
            plan.refusal = Some(error.message);
            return plan;
        }
    };
    // Read from the RESULT: on an arc the vertex is the point put on the arc.
    let added = result.vertices[s + 1];
    f.marks
        .push(vertex_mark(FeedbackRole::Added, added.position, ""));
    // Numbered as the vertices on screen are now: "vertex 2 between 1 and 2"
    // mixed the new vertex's number after the insert with its neighbours'
    // before it. The message after the click gives the new number.
    f.caption = format!(
        "new vertex between {a} and {b} \u{b7} {} from {a}",
        fixed(result.segment_length(s), 3)
    );
    if let Some(height) = added.height {
        f.caption
            .push_str(&format!(" \u{b7} height {}", fixed(height, 3)));
    }
    plan.message = format!(
        "vertex {} added to polyline {} on segment {s} ({})",
        s + 1,
        on.polyline,
        plural(result.vertices.len(), "vertex")
    );
    plan.edits.push(Edit {
        polyline: on.polyline,
        before: shape.clone(),
        after: result,
    });
    plan
}

fn plan_delete(c: &Collected, _cx: &ToolContext<'_>) -> Plan {
    let mut plan = Plan {
        command: String::from("VERTEX_DELETE"),
        ..Plan::default()
    };
    plan.feedback.focus = Some(c.picks[0].polyline);
    if c.picks.len() > 1 {
        // Several hot vertices, on one polyline or more: each polyline's
        // together, all in one step.
        let mut by_polyline: BTreeMap<EntityId, (CurvePolyline2, Vec<usize>)> = BTreeMap::new();
        for pick in &c.picks {
            let vertex = pick.vertex.unwrap_or_default();
            let entry = by_polyline
                .entry(pick.polyline)
                .or_insert_with(|| (pick.shape.clone(), Vec::new()));
            entry.1.push(vertex);
            plan.feedback.marks.push(vertex_mark(
                FeedbackRole::Removed,
                pick.shape.vertices[vertex].position,
                "",
            ));
        }
        for (id, (shape, indices)) in by_polyline {
            match delete_vertices(&shape, indices) {
                Ok(result) => plan.edits.push(Edit {
                    polyline: id,
                    before: shape,
                    after: result,
                }),
                Err(error) => {
                    plan.refusal = Some(format!("polyline {id}: {}", error.message));
                    return plan;
                }
            }
        }
        plan.feedback.caption = format!("delete {}", plural(c.picks.len(), "vertex"));
        plan.message = format!("{} deleted", plural(c.picks.len(), "vertex"));
        return plan;
    }
    let pick = &c.picks[0];
    let shape = &pick.shape;
    let v = pick.vertex.unwrap_or_default();
    let f = &mut plan.feedback;
    let result = match delete_vertex(shape, v) {
        Ok(result) => result,
        Err(error) => {
            f.marks.push(vertex_mark(
                FeedbackRole::Target,
                shape.vertices[v].position,
                format!("{v}"),
            ));
            plan.refusal = Some(error.message);
            return plan;
        }
    };
    let before = segment_before(shape, v);
    let after = segment_after(shape, v);
    if let Some(before) = before {
        f.marks.push(piece_mark(
            FeedbackRole::Removed,
            piece_of(shape, before),
            "",
        ));
    }
    if let Some(after) = after {
        f.marks.push(piece_mark(
            FeedbackRole::Removed,
            piece_of(shape, after),
            "",
        ));
    }
    f.marks.push(vertex_mark(
        FeedbackRole::Removed,
        shape.vertices[v].position,
        "",
    ));
    if let (Some(before), Some(after)) = (before, after) {
        // The segment that now joins the neighbours, read from the result: it
        // starts at the vertex before, which is the last one when the vertex
        // deleted was a closed polyline's first.
        let joined = if v == 0 {
            result.vertices.len() - 1
        } else {
            v - 1
        };
        f.shapes.push(piece_of(&result, joined));
        f.caption = format!("delete vertex {v} \u{b7} segments {before} and {after} become one");
    } else {
        f.caption = format!(
            "delete vertex {v} \u{b7} segment {} goes",
            before.or(after).unwrap_or_default()
        );
    }
    plan.message = format!("vertex {v} deleted");
    plan.edits.push(Edit {
        polyline: pick.polyline,
        before: shape.clone(),
        after: result,
    });
    plan
}

fn plan_move(c: &Collected, _cx: &ToolContext<'_>) -> Plan {
    let pick = &c.picks[0];
    let v = pick.vertex.unwrap_or_default();
    let from = pick.shape.vertices[v].position;
    let to = c.points[0];
    let mut plan = Plan {
        command: String::from("VERTEX_MOVE"),
        ..Plan::default()
    };
    let f = &mut plan.feedback;
    f.focus = Some(pick.polyline);
    f.marks
        .push(vertex_mark(FeedbackRole::Target, from, format!("{v}")));
    let result = move_vertex(&pick.shape, v, to).and_then(|moved| match c.height {
        Some(_) => set_vertex_height(&moved, v, c.height),
        None => Ok(moved),
    });
    let result = match result {
        Ok(result) => result,
        Err(error) => {
            plan.refusal = Some(error.message);
            return plan;
        }
    };
    f.marks.push(vertex_mark(
        FeedbackRole::Added,
        result.vertices[v].position,
        "",
    ));
    // The segments either side as they will be: an arc keeps its bulge.
    if let Some(before) = segment_before(&result, v) {
        f.shapes.push(piece_of(&result, before));
    }
    if let Some(after) = segment_after(&result, v) {
        f.shapes.push(piece_of(&result, after));
    }
    let distance = from.distance_to(to);
    f.caption = format!("vertex {v} \u{b7} moves {}", fixed(distance, 3));
    if distance > tolerance::GEOMETRIC {
        f.caption
            .push_str(&format!(" at {}", format_bearing((to - from).angle(), 0)));
    }
    if let Some(height) = c.height {
        f.caption
            .push_str(&format!(" \u{b7} height {}", fixed(height, 3)));
    }
    plan.message = format!("vertex {v} moved");
    plan.edits.push(Edit {
        polyline: pick.polyline,
        before: pick.shape.clone(),
        after: result,
    });
    plan
}

fn plan_edit(c: &Collected, _cx: &ToolContext<'_>) -> Plan {
    let pick = &c.picks[0];
    let mut plan = Plan::default();
    let f = &mut plan.feedback;
    f.focus = Some(pick.polyline);
    f.marks
        .push(piece_mark(FeedbackRole::Target, whole_of(&pick.shape), ""));
    f.caption = format!(
        "polyline {} \u{b7} {}, {}",
        pick.polyline,
        plural(pick.shape.vertices.len(), "vertex"),
        if pick.shape.closed { "closed" } else { "open" }
    );
    plan.selection = Some(vec![pick.polyline]);
    plan.message = format!(
        "{} in the Vertices panel",
        plural(pick.shape.vertices.len(), "vertex")
    );
    plan
}

// Straighten and Grade walk their range by vertex_editing's vertex_range, the
// rule the STRAIGHTEN and VERTEXZ GRADE verbs walk too.

fn plan_straighten(c: &Collected, _cx: &ToolContext<'_>) -> Plan {
    let first = &c.picks[0];
    let second = &c.picks[1];
    let shape = &first.shape;
    let a = first.vertex.unwrap_or_default();
    let b = second.vertex.unwrap_or_default();
    let mut plan = Plan {
        command: String::from("STRAIGHTEN"),
        ..Plan::default()
    };
    let f = &mut plan.feedback;
    f.focus = Some(first.polyline);
    f.marks.push(vertex_mark(
        FeedbackRole::Target,
        shape.vertices[a].position,
        "keep",
    ));
    f.marks.push(vertex_mark(
        FeedbackRole::Target,
        shape.vertices[b].position,
        "keep",
    ));
    let (from, to) = vertex_range(shape, a, b, c.other_side);
    let result = match straighten(shape, from, to) {
        Ok(result) => result,
        Err(error) => {
            plan.refusal = Some(error.message);
            return plan;
        }
    };
    let removed = vertices_between(shape, from, to);
    for segment in segments_between(shape, from, to) {
        f.marks.push(piece_mark(
            FeedbackRole::Removed,
            piece_of(shape, segment),
            "",
        ));
    }
    for &vertex in &removed {
        f.marks.push(vertex_mark(
            FeedbackRole::Removed,
            shape.vertices[vertex].position,
            "",
        ));
    }
    // The one straight segment left, from the result: it starts where `from`
    // now is, which is earlier by the vertices removed before it.
    let shift = removed.iter().filter(|&&i| i < from).count();
    f.shapes.push(piece_of(&result, from - shift));
    f.caption = format!(
        "straighten {from} to {to} \u{b7} removes {}",
        plural(removed.len(), "vertex")
    );
    plan.message = format!("{} removed", plural(removed.len(), "vertex"));
    plan.edits.push(Edit {
        polyline: first.polyline,
        before: shape.clone(),
        after: result,
    });
    plan
}

fn plan_grade(c: &Collected, _cx: &ToolContext<'_>) -> Plan {
    let first = &c.picks[0];
    let second = &c.picks[1];
    let shape = &first.shape;
    let a = first.vertex.unwrap_or_default();
    let b = second.vertex.unwrap_or_default();
    let mut plan = Plan {
        command: String::from("GRADE"),
        ..Plan::default()
    };
    let f = &mut plan.feedback;
    f.focus = Some(first.polyline);
    f.marks.push(vertex_mark(
        FeedbackRole::Target,
        shape.vertices[a].position,
        height_text(shape.vertices[a].height),
    ));
    f.marks.push(vertex_mark(
        FeedbackRole::Target,
        shape.vertices[b].position,
        height_text(shape.vertices[b].height),
    ));
    let (from, to) = vertex_range(shape, a, b, c.other_side);
    let result = match grade_between(shape, from, to) {
        Ok(result) => result,
        Err(error) => {
            plan.refusal = Some(error.message);
            return plan;
        }
    };
    let between = vertices_between(shape, from, to);
    for &vertex in &between {
        f.marks.push(vertex_mark(
            FeedbackRole::Target,
            shape.vertices[vertex].position,
            format!(
                "\u{2192} {}",
                fixed(result.vertices[vertex].height.unwrap_or_default(), 3)
            ),
        ));
    }
    let mut length = 0.0;
    for segment in segments_between(shape, from, to) {
        length += shape.segment_length(segment);
    }
    let za = shape.vertices[a].height.unwrap_or_default();
    let zb = shape.vertices[b].height.unwrap_or_default();
    f.caption = format!(
        "grade {a} \u{2192} {b}: {}, {} to {}",
        plural(between.len(), "vertex"),
        fixed(za, 3),
        fixed(zb, 3)
    );
    if length > tolerance::GEOMETRIC {
        f.caption
            .push_str(&format!(" ({} %)", fixed((zb - za) / length * 100.0, 3)));
    }
    plan.message = format!("graded from vertex {a} to {b}");
    plan.edits.push(Edit {
        polyline: first.polyline,
        before: shape.clone(),
        after: result,
    });
    plan
}

fn plan_start(c: &Collected, _cx: &ToolContext<'_>) -> Plan {
    let pick = &c.picks[0];
    let v = pick.vertex.unwrap_or_default();
    let mut plan = Plan {
        command: String::from("START_VERTEX"),
        ..Plan::default()
    };
    plan.feedback.focus = Some(pick.polyline);
    let result = change_start_vertex(&pick.shape, v);
    // Named by its number where it is refused: "new 0" beside the struck
    // ring promised the start the caption turned down.
    plan.feedback.marks.push(vertex_mark(
        FeedbackRole::Target,
        pick.shape.vertices[v].position,
        if result.is_ok() {
            String::from("new 0")
        } else {
            format!("{v}")
        },
    ));
    let result = match result {
        Ok(result) => result,
        Err(error) => {
            plan.refusal = Some(error.message);
            return plan;
        }
    };
    plan.feedback.caption = format!("vertex {v} becomes vertex 0");
    plan.message = format!("vertex {v} is now the start");
    plan.edits.push(Edit {
        polyline: pick.polyline,
        before: pick.shape.clone(),
        after: result,
    });
    plan
}

fn plan_height(c: &Collected, _cx: &ToolContext<'_>) -> Plan {
    let pick = &c.picks[0];
    let v = pick.vertex.unwrap_or_default();
    let current = pick.shape.vertices[v].height;
    let mut plan = Plan {
        command: String::from("VERTEX_Z"),
        ..Plan::default()
    };
    let f = &mut plan.feedback;
    f.focus = Some(pick.polyline);
    f.marks.push(vertex_mark(
        FeedbackRole::Target,
        pick.shape.vertices[v].position,
        height_text(current),
    ));
    let said = |z: Option<f64>| match z {
        Some(z) => format!("height {}", fixed(z, 3)),
        None => String::from("no height"),
    };
    f.caption = format!("vertex {v} \u{b7} {}", said(current));
    let text = &c.values[0];
    let mut height = None;
    if !is_option(text, "None") {
        let Some(typed) = parse_finite_double(text) else {
            plan.warning = Some(format!("'{text}' is not a height; type a number or None"));
            return plan;
        };
        height = Some(typed);
    }
    // The height it will have, where that is a change: the height a click at
    // the value step takes, or Enter's default, was nowhere on screen.
    if height != current {
        f.caption.push_str(&format!(
            " \u{2192} {}",
            match height {
                Some(height) => fixed(height, 3),
                None => String::from("no height"),
            }
        ));
    }
    if height == current {
        // Enter at a vertex's own height: nothing to do, and no undo step.
        plan.no_change = Some(format!(
            "vertex {v} keeps {}",
            match current {
                Some(current) => format!("its height {}", number(current)),
                None => String::from("no height"),
            }
        ));
        return plan;
    }
    let result = match set_vertex_height(&pick.shape, v, height) {
        Ok(result) => result,
        Err(error) => {
            plan.warning = Some(error.message);
            return plan;
        }
    };
    plan.message = format!(
        "vertex {v}{}",
        match height {
            Some(height) => format!(" at {}", number(height)),
            None => String::from(" has no height"),
        }
    );
    plan.edits.push(Edit {
        polyline: pick.polyline,
        before: pick.shape.clone(),
        after: result,
    });
    plan
}

fn plan_segment_arc(c: &Collected, _cx: &ToolContext<'_>) -> Plan {
    let pick = &c.picks[0];
    let s = pick.segment.unwrap_or_default();
    let mut plan = Plan {
        command: String::from("SEGMENT_ARC"),
        ..Plan::default()
    };
    let f = &mut plan.feedback;
    f.focus = Some(pick.polyline);
    f.marks.push(piece_mark(
        FeedbackRole::Target,
        piece_of(&pick.shape, s),
        "",
    ));
    let result = match segment_to_arc(&pick.shape, s, c.points[0]) {
        Ok(result) => result,
        Err(error) => {
            plan.refusal = Some(error.message);
            return plan;
        }
    };
    f.shapes.push(piece_of(&result, s));
    let arc = arc_from_bulge(
        result.vertices[s].position,
        result.vertices[result.segment_end(s)].position,
        result.vertices[s].bulge,
    );
    f.caption = format!(
        "segment {s}{}",
        match arc {
            Some(arc) => format!(" \u{b7} radius {}", fixed(arc.radius, 3)),
            None => String::new(),
        }
    );
    plan.message = format!("segment {s} is an arc");
    plan.edits.push(Edit {
        polyline: pick.polyline,
        before: pick.shape.clone(),
        after: result,
    });
    plan
}

fn plan_segment_line(c: &Collected, _cx: &ToolContext<'_>) -> Plan {
    let pick = &c.picks[0];
    let s = pick.segment.unwrap_or_default();
    let mut plan = Plan {
        command: String::from("SEGMENT_LINE"),
        ..Plan::default()
    };
    let f = &mut plan.feedback;
    f.focus = Some(pick.polyline);
    f.marks.push(piece_mark(
        FeedbackRole::Target,
        piece_of(&pick.shape, s),
        "",
    ));
    let result = match segment_to_line(&pick.shape, s) {
        Ok(result) => result,
        Err(error) => {
            plan.refusal = Some(error.message);
            return plan;
        }
    };
    f.shapes.push(piece_of(&result, s));
    f.caption = format!("segment {s} becomes straight");
    plan.message = format!("segment {s} is straight");
    plan.edits.push(Edit {
        polyline: pick.polyline,
        before: pick.shape.clone(),
        after: result,
    });
    plan
}

// The piece of `result` that replaced corner `v` of `shape`: it starts at the
// vertex before the corner when the cut reached that vertex, else where the
// corner was (geometry's polyline_vertices, replace_corner, which drops a
// cut point that lands on a neighbour rather than doubling it).
fn corner_piece(shape: &CurvePolyline2, result: &CurvePolyline2, v: usize, arc: bool) -> usize {
    let at_corner = v;
    let before_corner = if v > 0 {
        v - 1
    } else {
        result.vertices.len().wrapping_sub(1)
    };
    if arc {
        return if at_corner < result.segment_count() && result.is_arc(at_corner) {
            at_corner
        } else {
            before_corner
        };
    }
    // A bevel: both cut points new (one more vertex than before), or the
    // first one new (a vertex on the incoming segment where the corner was).
    // Unsigned as the C++'s size_t arithmetic is.
    let replacement = (result.vertices.len() + 1).wrapping_sub(shape.vertices.len());
    if replacement == 2 {
        return at_corner;
    }
    if replacement == 1 {
        let previous = segment_before(shape, v).unwrap_or(0);
        let incoming = Segment2::new(
            shape.vertices[previous].position,
            shape.vertices[v].position,
        );
        return if incoming.distance_to(result.vertices[at_corner].position) <= tolerance::GEOMETRIC
        {
            at_corner
        } else {
            before_corner
        };
    }
    before_corner
}

fn plan_fillet(c: &Collected, _cx: &ToolContext<'_>) -> Plan {
    let pick = &c.picks[0];
    let v = pick.vertex.unwrap_or_default();
    let shape = &pick.shape;
    let mut plan = Plan {
        command: String::from("FILLET_VERTEX"),
        ..Plan::default()
    };
    let f = &mut plan.feedback;
    f.focus = Some(pick.polyline);
    if let Err(error) = check_corner(shape, v) {
        // The vertex that is no corner, named: drawn struck through.
        f.marks.push(vertex_mark(
            FeedbackRole::Target,
            shape.vertices[v].position,
            format!("{v}"),
        ));
        plan.refusal = Some(error.message);
        return plan;
    }
    // What the pick acts on is the two segments the arc is tangent to, as
    // Chamfer marks its two; the corner itself goes (its X, below). A ring on
    // the corner with the X over it was a circled X - "cancel" - and a near
    // twin of the refusal's struck ring, which at the default radius is all
    // the preview showed.
    f.marks.push(piece_mark(
        FeedbackRole::Target,
        piece_of(shape, segment_before(shape, v).unwrap_or_default()),
        "",
    ));
    f.marks.push(piece_mark(
        FeedbackRole::Target,
        piece_of(shape, segment_after(shape, v).unwrap_or_default()),
        "",
    ));
    let text = &c.values[0];
    let Some(radius) = parse_positive(text) else {
        plan.warning = Some(format!("'{text}' is not a radius greater than zero"));
        return plan;
    };
    let result = match fillet_vertex(shape, v, radius) {
        Ok(result) => result,
        Err(error) => {
            plan.warning = Some(error.message);
            return plan;
        }
    };
    let piece = corner_piece(shape, &result, v, true);
    f.marks.push(vertex_mark(
        FeedbackRole::Removed,
        shape.vertices[v].position,
        "",
    ));
    f.marks.push(vertex_mark(
        FeedbackRole::Added,
        result.vertices[piece].position,
        "",
    ));
    f.marks.push(vertex_mark(
        FeedbackRole::Added,
        result.vertices[result.segment_end(piece)].position,
        "",
    ));
    f.shapes.push(piece_of(&result, piece));
    f.caption = format!("fillet vertex {v} \u{b7} radius {}", fixed(radius, 3));
    plan.message = format!("vertex {v} filleted at {}", number(radius));
    plan.edits.push(Edit {
        polyline: pick.polyline,
        before: shape.clone(),
        after: result,
    });
    plan
}

fn plan_chamfer(c: &Collected, _cx: &ToolContext<'_>) -> Plan {
    let pick = &c.picks[0];
    let v = pick.vertex.unwrap_or_default();
    let shape = &pick.shape;
    let mut plan = Plan {
        command: String::from("CHAMFER_VERTEX"),
        ..Plan::default()
    };
    let f = &mut plan.feedback;
    f.focus = Some(pick.polyline);
    if let Err(error) = check_corner(shape, v) {
        // The vertex that is no corner, named and struck, as Fillet's: the
        // one segment an end has was struck instead, labelled "d1" - a
        // distance the refusal never asks for.
        f.marks.push(vertex_mark(
            FeedbackRole::Target,
            shape.vertices[v].position,
            format!("{v}"),
        ));
        plan.refusal = Some(error.message);
        return plan;
    }
    // "d1" and "d2", the distances measured along each: a bare "1" and "2" on
    // the segments read as segment numbers beside the vertex numbers.
    f.marks.push(piece_mark(
        FeedbackRole::Target,
        piece_of(shape, segment_before(shape, v).unwrap_or_default()),
        "d1",
    ));
    f.marks.push(piece_mark(
        FeedbackRole::Target,
        piece_of(shape, segment_after(shape, v).unwrap_or_default()),
        "d2",
    ));
    let (Some(first), Some(second)) = (parse_positive(&c.values[0]), parse_positive(&c.values[1]))
    else {
        plan.warning = Some(String::from("type two distances greater than zero"));
        return plan;
    };
    let result = match chamfer_vertex(shape, v, first, second) {
        Ok(result) => result,
        Err(error) => {
            plan.warning = Some(error.message);
            return plan;
        }
    };
    let piece = corner_piece(shape, &result, v, false);
    f.marks.push(vertex_mark(
        FeedbackRole::Removed,
        shape.vertices[v].position,
        "",
    ));
    f.marks.push(vertex_mark(
        FeedbackRole::Added,
        result.vertices[piece].position,
        "",
    ));
    f.marks.push(vertex_mark(
        FeedbackRole::Added,
        result.vertices[result.segment_end(piece)].position,
        "",
    ));
    f.shapes.push(piece_of(&result, piece));
    f.caption = format!(
        "chamfer vertex {v} \u{b7} {} and {}",
        fixed(first, 3),
        fixed(second, 3)
    );
    plan.message = format!("vertex {v} chamfered");
    plan.edits.push(Edit {
        polyline: pick.polyline,
        before: shape.clone(),
        after: result,
    });
    plan
}

// ---- what the handles answer, tool by tool ---------------------------------------------------

// Insert: nothing is answered. The handle says WHERE (beside a hot vertex,
// on a hot segment) and the click or Enter says the point.
fn answer_insert(cx: &ToolContext<'_>, start: &ToolStart, c: &mut Collected) -> usize {
    let mut segments = handles_of(cx, &start.handles, GripKind::SegmentMid);
    let mut hot = handles_of(cx, &start.handles, GripKind::Vertex);
    if segments.len() == 1 && hot.is_empty() {
        c.anchor = segments.pop();
    } else if segments.is_empty() && hot.len() == 1 {
        c.anchor = hot.pop();
    } else if segments.is_empty() && hot.len() == 2 {
        c.anchor = segment_between(&hot[0], &hot[1]);
        if c.anchor.is_none() {
            c.note = no_segment_between(&hot[0], &hot[1]);
        }
    } else if segments.len() + hot.len() > 1 {
        c.note = format!(
            "{} grips are chosen; a new vertex goes beside one vertex, or on one segment",
            segments.len() + hot.len()
        );
    }
    0
}

fn prompt_insert(c: &Collected, in_play: Option<EntityId>) -> String {
    // Enter's offer first, as every tool's "Press Enter to ..." with
    // something chosen: the command line cuts a long placeholder at the
    // right, and it cut "or press En...", the half that answers "what about
    // the already selected vertex".
    if let Some(anchor) = &c.anchor
        && let Some(vertex) = anchor.vertex
    {
        let middle = segment_after(&anchor.shape, vertex)
            .or_else(|| segment_before(&anchor.shape, vertex))
            .unwrap_or(0);
        return format!(
            "Press Enter for a vertex at the marked middle of segment {middle}, or click beside \
             vertex {vertex} of polyline {}",
            anchor.polyline
        );
    }
    if let Some(anchor) = &c.anchor
        && let Some(segment) = anchor.segment
    {
        return format!(
            "Press Enter for a vertex at the marked middle of segment {segment} of polyline {}, or \
             click on it",
            anchor.polyline
        );
    }
    match in_play {
        Some(id) => format!("Click on polyline {id} where the new vertex goes"),
        None => String::from("Click on a polyline where the new vertex goes"),
    }
}

fn answer_delete(cx: &ToolContext<'_>, start: &ToolStart, c: &mut Collected) -> usize {
    let hot = handles_of(cx, &start.handles, GripKind::Vertex);
    if hot.is_empty() {
        return 0;
    }
    c.picks = hot;
    1
}

fn ready_delete(c: &Collected, _document: Option<&Document>) -> String {
    if c.picks.len() == 1 {
        return format!(
            "Press Enter to delete {}, or click another vertex",
            picked_vertex(c, 0)
        );
    }
    format!(
        "Press Enter to delete the {} chosen vertices, or click another vertex",
        c.picks.len()
    )
}

fn answer_move(cx: &ToolContext<'_>, start: &ToolStart, c: &mut Collected) -> usize {
    let hot = handles_of(cx, &start.handles, GripKind::Vertex);
    if hot.len() == 1 {
        c.picks = hot;
        return 1;
    }
    if hot.len() > 1 {
        c.note = format!("{} vertices are chosen, and this tool moves one", hot.len());
    }
    0
}

// Edit Vertices: a grip chosen, else the one polyline selected: it is in the
// panel already, so the tool does not ask for it again. Only what the view
// lets be edited (R1), as every other vertex tool: one whose layer was
// hidden or locked after it was selected was said to be "in the Vertices
// panel" for editing.
fn answer_edit(cx: &ToolContext<'_>, start: &ToolStart, c: &mut Collected) -> usize {
    let usable = |id: EntityId| {
        cx.document
            .and_then(|document| document.model().entities.find(id))
            .is_some_and(|entity| is_polyline_entity(entity) && pickable(cx, entity))
    };
    let mut id = start
        .handles
        .iter()
        .map(|grip| grip.entity)
        .find(|&entity| usable(entity))
        .unwrap_or(INVALID_ENTITY_ID);
    if id == INVALID_ENTITY_ID {
        let mut own = polylines_among(cx.document, &start.selection);
        own.retain(|&candidate| usable(candidate));
        if own.len() == 1 {
            id = own[0];
        }
    }
    let Some(shape) = polyline_now(cx.document, id) else {
        return 0;
    };
    let at = shape
        .vertices
        .first()
        .map(|vertex| vertex.position)
        .unwrap_or_default();
    c.picks = vec![Chosen {
        polyline: id,
        shape,
        at,
        from_handle: true,
        ..Chosen::default()
    }];
    1
}

fn ready_edit(c: &Collected, document: Option<&Document>) -> String {
    // The panel shows the first polyline of the selection (vertex_panel.cpp,
    // refresh); a grip of another selected one is put there by Enter, which
    // selects it.
    let id = c.picks[0].polyline;
    let selected = match document {
        Some(document) => polylines_among(Some(document), &document.selection().ids()),
        None => Vec::new(),
    };
    if selected.first() == Some(&id) {
        return format!(
            "Polyline {id} is in the Vertices panel; click another polyline, or press Enter"
        );
    }
    format!("Press Enter to show polyline {id} in the Vertices panel, or click another polyline")
}

fn ready_straighten(c: &Collected, _document: Option<&Document>) -> String {
    format!(
        "Press Enter to straighten polyline {}{}, or click another vertex{}",
        c.picks[0].polyline,
        walked_between(c),
        if c.picks[0].shape.closed {
            " [Other side]"
        } else {
            ""
        }
    )
}

fn ready_start(c: &Collected, _document: Option<&Document>) -> String {
    format!(
        "Press Enter to make vertex {} the start of polyline {}, or click another vertex",
        c.picks[0].vertex.unwrap_or_default(),
        c.picks[0].polyline
    )
}

// Set Height's value step: its own height, so Enter changes nothing; else
// the height interpolated along the polyline; else none.
fn height_fallback(c: &Collected) -> Option<String> {
    let pick = c.picks.first()?;
    let v = pick.vertex.unwrap_or_default();
    if let Some(height) = pick.shape.vertices[v].height {
        return Some(number(height));
    }
    if let Ok(filled) = interpolate_heights(&pick.shape)
        && let Some(height) = filled.vertices[v].height
    {
        return Some(number(height));
    }
    Some(String::from("None"))
}

// Set Height: only what the view draws gives a height to a click - a hidden
// point's height is not one the user can see to click.
fn height_from_click(_c: &Collected, cx: &ToolContext<'_>, at: Point2) -> Option<String> {
    let document = cx.document?;
    height_at_point(document, at, vertex_reach(cx), view_of(cx)).map(number)
}

fn has_height(chosen: &Chosen) -> Option<String> {
    let v = chosen.vertex.unwrap_or_default();
    if chosen.shape.vertices[v].height.is_some() {
        return None;
    }
    Some(format!(
        "vertex {v} has no height; grading needs one at both ends"
    ))
}

fn height_label(chosen: &Chosen) -> String {
    height_text(chosen.shape.vertices[chosen.vertex.unwrap_or_default()].height)
}

fn ready_grade(c: &Collected, _document: Option<&Document>) -> String {
    format!(
        "Press Enter to grade polyline {}{}, or click another vertex{}",
        c.picks[0].polyline,
        walked_between(c),
        if c.picks[0].shape.closed {
            " [Other side]"
        } else {
            ""
        }
    )
}

// Refused at the pick (R3): a segment with no length has no arc through any
// point, and taken, every point after it was refused and only Esc or U got
// out.
fn has_length(chosen: &Chosen) -> Option<String> {
    let s = chosen.segment.unwrap_or_default();
    if chosen.shape.segment_length(s) > tolerance::GEOMETRIC {
        return None;
    }
    Some(format!(
        "segment {s} has no length; an arc needs its two ends apart"
    ))
}

fn ready_segment_line(c: &Collected, _document: Option<&Document>) -> String {
    format!(
        "Press Enter to straighten segment {} of polyline {}, or click another segment",
        c.picks[0].segment.unwrap_or_default(),
        c.picks[0].polyline
    )
}

fn remember_fillet(c: &Collected) {
    if let Some(radius) = parse_positive(&c.values[0]) {
        keep_defaults(|kept| kept.fillet_radius = radius);
    }
}

fn remember_chamfer(c: &Collected) {
    if let (Some(first), Some(second)) =
        (parse_positive(&c.values[0]), parse_positive(&c.values[1]))
    {
        keep_defaults(|kept| {
            kept.chamfer_first = first;
            kept.chamfer_second = second;
        });
    }
}

// Chamfer's distance prompts: along the segment into the corner, then out.
fn along(which: &str, incoming: bool) -> PromptFn {
    let which = String::from(which);
    Box::new(move |c: &Collected, _in_play: Option<EntityId>| {
        let pick = &c.picks[0];
        let vertex = pick.vertex.unwrap_or_default();
        let segment = if incoming {
            segment_before(&pick.shape, vertex)
        } else {
            segment_after(&pick.shape, vertex)
        };
        format!(
            "Specify the {which} distance, along segment {}",
            segment.unwrap_or(0)
        )
    })
}

// ---- registration -------------------------------------------------------------------------

fn info(id: &str, name: &str, order: i32, aliases: &[&str], tip: &str, script: Script) -> ToolInfo {
    // The prompt names the tool by what it does; the menus gather the family
    // by its name's "Vertices, " (tool_menus.hpp).
    let script = Arc::new(Script {
        title: String::from(name),
        ..script
    });
    ToolInfo {
        id: String::from(id),
        name: format!("Vertices, {name}"),
        title: String::from(name),
        category: String::from("Draw"),
        group: String::from("Vertices"),
        order,
        aliases: aliases.iter().map(|&alias| String::from(alias)).collect(),
        shortcut: String::new(),
        tip: String::from(tip),
        make: Some(factory(move |cx, start| {
            Box::new(ScriptTool::new(cx, start, Arc::clone(&script)))
        })),
    }
}

// A selection tool's script: select, then the values, then `finish`.
fn selection_script(steps: Vec<Step>, finish: FinishFn) -> Script {
    Script {
        steps,
        finish: Some(finish),
        ..Script::default()
    }
}

/// Draw > Vertices: vertex control of polylines (docs/drawing.md).
pub fn add_modify_vertex_tools(catalog: &mut ToolCatalog, report: &mut Report<'_>) {
    report(catalog.add(info(
        "draw.vertex.insert",
        "Insert Vertex",
        10,
        &["INSERTVERTEX"],
        "Adds a vertex where you click on a polyline - always on the line, on an arc on the arc - \
         shown before the click; with a vertex chosen first, beside it.",
        Script {
            steps: vec![Step {
                // x,y,z keeps z as the new vertex's height.
                heights: true,
                ..Step::new(StepKind::OnLine, Box::new(prompt_insert))
            }],
            plan: Some(plan_insert),
            answer: Some(answer_insert),
            restarts: true,
            ..Script::default()
        },
    )));

    report(catalog.add(info(
        "draw.vertex.delete",
        "Delete Vertex",
        20,
        &["DELETEVERTEX"],
        "Removes the vertex you click, or the vertices chosen before; its two segments become \
         one.",
        Script {
            steps: vec![vertex_step(click_the("vertex", "to delete"))],
            plan: Some(plan_delete),
            answer: Some(answer_delete),
            ready: Some(ready_delete),
            restarts: true,
            delete_is_enter: true,
            ..Script::default()
        },
    )));

    report(catalog.add(info(
        "draw.vertex.move",
        "Move Vertex",
        30,
        &["MOVEVERTEX"],
        "Moves the vertex you click, or the one chosen before, to a picked or typed point; \
         @dx,dy is from the vertex, and @dx,dy,dz raises it by dz.",
        Script {
            steps: vec![
                vertex_step(click_the("vertex", "to move")),
                Step {
                    // x,y,z sets its height, @dx,dy,dz changes it.
                    heights: true,
                    ..point_step("Specify its new position")
                },
            ],
            plan: Some(plan_move),
            answer: Some(answer_move),
            restarts: true,
            ..Script::default()
        },
    )));

    report(catalog.add(info(
        "draw.vertex.edit",
        "Edit Vertices",
        40,
        &["EDITVERTICES", "VERTEX"],
        "Shows a polyline's vertices in the Vertices panel, to edit in place: the one selected, \
         or the one you click.",
        Script {
            steps: vec![Step::new(
                StepKind::Polyline,
                fixed_prompt("Click the polyline whose vertices to edit"),
            )],
            plan: Some(plan_edit),
            answer: Some(answer_edit),
            ready: Some(ready_edit),
            ..Script::default()
        },
    )));

    report(catalog.add(info(
        "draw.vertex.straighten",
        "Straighten",
        50,
        &["STRAIGHTEN"],
        "Removes every vertex between two you click, leaving one straight segment between them; \
         on a closed polyline the side with fewer, or the other side with O.",
        Script {
            steps: vec![
                Step {
                    label: Some(Box::new(|_chosen: &Chosen| String::from("keep"))),
                    ..vertex_step(click_the("first vertex", "to keep"))
                },
                Step {
                    same_polyline: true,
                    other_side: true,
                    ..vertex_step(fixed_prompt("Click the other vertex to keep"))
                },
            ],
            plan: Some(plan_straighten),
            answer: Some(two_vertices),
            ready: Some(ready_straighten),
            restarts: true,
            ..Script::default()
        },
    )));

    report(catalog.add(info(
        "draw.vertex.weed",
        "Weed",
        60,
        &["WEED", "SIMPLIFY"],
        "Removes vertices that lie within a tolerance of the line through their neighbours \
         (Douglas-Peucker), in plan and height, optionally keeping vertices on survey points.",
        selection_script(
            vec![
                select_polylines("weed"),
                value_step(
                    fixed_prompt("Specify the weed tolerance"),
                    remembered(|kept| kept.weed_tolerance),
                ),
                value_step(
                    fixed_prompt("Keep vertices on survey points? [Yes/No]"),
                    Some(Box::new(|_c: &Collected| {
                        Some(String::from(if defaults().weed_keeps_survey_points {
                            "Yes"
                        } else {
                            "No"
                        }))
                    })),
                ),
            ],
            Box::new(|c: &Collected| {
                let Some(tolerance) = parse_non_negative(&c.values[0]) else {
                    return ToolStep::rejected(format!(
                        "'{}' is not a tolerance; type a distance of zero or more",
                        c.values[0]
                    ));
                };
                let keep = if is_option(&c.values[1], "No") {
                    false
                } else if is_option(&c.values[1], "Yes") {
                    true
                } else {
                    return ToolStep::rejected("type Yes or No");
                };
                keep_defaults(|kept| {
                    kept.weed_tolerance = tolerance;
                    kept.weed_keeps_survey_points = keep;
                });
                ToolStep::done(
                    Some(edit_each_polyline(
                        c.selection.clone(),
                        "WEED",
                        move |model: &Model, _id: EntityId, polyline: &CurvePolyline2| {
                            let survey_points = if keep {
                                vertices_on_survey_points(model, polyline, SURVEY_POINT_TOLERANCE)
                            } else {
                                Vec::new()
                            };
                            weed(polyline, tolerance, &survey_points)
                        },
                    )),
                    format!(
                        "{} weeded to {}",
                        plural(c.selection.len(), "polyline"),
                        number(tolerance)
                    ),
                    false,
                )
            }),
        ),
    )));

    report(catalog.add(info(
        "draw.vertex.densify",
        "Densify",
        70,
        &["DENSIFY"],
        "Adds vertices so no straight segment is longer than an interval, and optionally turns \
         arcs into chords within a tolerance.",
        selection_script(
            vec![
                select_polylines("densify"),
                value_step(
                    fixed_prompt("Specify the interval, or 0 to leave straight segments"),
                    remembered(|kept| kept.densify_interval),
                ),
                value_step(
                    fixed_prompt("Specify the chord tolerance for arcs, or 0 to keep them as arcs"),
                    remembered(|kept| kept.densify_chord),
                ),
            ],
            Box::new(|c: &Collected| {
                let (Some(step), Some(chord)) = (
                    parse_non_negative(&c.values[0]),
                    parse_non_negative(&c.values[1]),
                ) else {
                    return ToolStep::rejected("type distances of zero or more");
                };
                keep_defaults(|kept| {
                    kept.densify_interval = step;
                    kept.densify_chord = chord;
                });
                ToolStep::done(
                    Some(edit_polylines(
                        c.selection.clone(),
                        "DENSIFY",
                        move |polyline: &CurvePolyline2| densify(polyline, step, chord),
                    )),
                    format!("{} densified", plural(c.selection.len(), "polyline")),
                    false,
                )
            }),
        ),
    )));

    report(catalog.add(info(
        "draw.vertex.close",
        "Close or Open",
        80,
        &["CLOSEOPEN"],
        "Closes each open polyline selected and opens each closed one.",
        selection_script(
            vec![select_polylines("close or open")],
            Box::new(|c: &Collected| {
                ToolStep::done(
                    Some(edit_polylines(
                        c.selection.clone(),
                        "CLOSE_OPEN",
                        |polyline: &CurvePolyline2| {
                            if polyline.closed {
                                open_polyline(polyline)
                            } else {
                                close_polyline(polyline)
                            }
                        },
                    )),
                    format!("{} closed or opened", plural(c.selection.len(), "polyline")),
                    false,
                )
            }),
        ),
    )));

    report(catalog.add(info(
        "draw.vertex.start",
        "Change Start Vertex",
        90,
        &["STARTVERTEX"],
        "Makes the vertex you click the first of a closed polyline; its shape is unchanged.",
        Script {
            steps: vec![Step {
                label: Some(Box::new(|_chosen: &Chosen| String::from("new 0"))),
                ..vertex_step(Box::new(
                    |_c: &Collected, in_play: Option<EntityId>| match in_play {
                        Some(id) => format!("Click the vertex of polyline {id} to be its start"),
                        None => {
                            String::from("Click the vertex to be the start of a closed polyline")
                        }
                    },
                ))
            }],
            plan: Some(plan_start),
            answer: Some(one_vertex),
            ready: Some(ready_start),
            restarts: true,
            ..Script::default()
        },
    )));

    report(catalog.add(info(
        "draw.vertex.height",
        "Set Vertex Height",
        100,
        &["VERTEXZ"],
        "Sets the height of the vertex you click - typed, or clicked from a point that has one - \
         or clears it with None.",
        Script {
            steps: vec![
                Step {
                    label: Some(Box::new(height_label)),
                    ..vertex_step(click_the("vertex", "whose height to set"))
                },
                value_step(
                    Box::new(|c: &Collected, _in_play: Option<EntityId>| {
                        format!(
                            "Specify the height of vertex {} or [None]",
                            c.picks[0].vertex.unwrap_or_default()
                        )
                    }),
                    Some(Box::new(height_fallback)),
                ),
            ],
            plan: Some(plan_height),
            answer: Some(one_vertex),
            value_from_click: Some(height_from_click),
            restarts: true,
            ..Script::default()
        },
    )));

    report(catalog.add(info(
        "draw.vertex.interpolate",
        "Interpolate Heights",
        110,
        &["INTERPOLATEZ"],
        "Gives every vertex without a height the height interpolated by length between the \
         vertices either side that have one.",
        selection_script(
            vec![select_polylines("interpolate the heights of")],
            Box::new(|c: &Collected| {
                ToolStep::done(
                    Some(edit_polylines(
                        c.selection.clone(),
                        "INTERPOLATE_Z",
                        interpolate_heights,
                    )),
                    format!("{} interpolated", plural(c.selection.len(), "polyline")),
                    false,
                )
            }),
        ),
    )));

    report(catalog.add(info(
        "draw.vertex.grade",
        "Grade Between Vertices",
        120,
        &["GRADE"],
        "Sets the heights of the vertices between two you click on the straight grade between \
         their heights.",
        Script {
            steps: vec![
                Step {
                    check: Some(Box::new(has_height)),
                    label: Some(Box::new(height_label)),
                    ..vertex_step(click_the("vertex", "to grade from"))
                },
                Step {
                    same_polyline: true,
                    other_side: true,
                    check: Some(Box::new(has_height)),
                    ..vertex_step(fixed_prompt("Click the vertex to grade to"))
                },
            ],
            plan: Some(plan_grade),
            answer: Some(two_vertices),
            ready: Some(ready_grade),
            restarts: true,
            ..Script::default()
        },
    )));

    report(catalog.add(info(
        "draw.vertex.arc",
        "Segment to Arc",
        130,
        &["SEGMENTARC"],
        "Makes the segment you click an arc through a point you give.",
        Script {
            steps: vec![
                Step {
                    check: Some(Box::new(has_length)),
                    ..segment_step(click_the("segment", "to make an arc"))
                },
                point_step("Specify a point the arc passes through"),
            ],
            plan: Some(plan_segment_arc),
            answer: Some(one_segment),
            restarts: true,
            ..Script::default()
        },
    )));

    report(catalog.add(info(
        "draw.vertex.line",
        "Segment to Line",
        140,
        &["SEGMENTLINE"],
        "Makes the arc segment you click straight.",
        Script {
            steps: vec![segment_step(click_the("arc segment", "to straighten"))],
            plan: Some(plan_segment_line),
            answer: Some(one_segment),
            ready: Some(ready_segment_line),
            restarts: true,
            ..Script::default()
        },
    )));

    report(catalog.add(info(
        "draw.vertex.fillet",
        "Fillet Vertex",
        150,
        &["FILLETVERTEX"],
        "Rounds the corner at the vertex you click with a tangent arc of a radius, shown at the \
         last radius before you click.",
        Script {
            steps: vec![
                vertex_step(click_the("corner vertex", "to round")),
                value_step(
                    Box::new(|c: &Collected, _in_play: Option<EntityId>| {
                        format!(
                            "Specify the fillet radius at vertex {}",
                            c.picks[0].vertex.unwrap_or_default()
                        )
                    }),
                    remembered(|kept| kept.fillet_radius),
                ),
            ],
            plan: Some(plan_fillet),
            answer: Some(one_vertex),
            remember: Some(remember_fillet),
            restarts: true,
            ..Script::default()
        },
    )));

    report(catalog.add(info(
        "draw.vertex.chamfer",
        "Chamfer Vertex",
        160,
        &["CHAMFERVERTEX"],
        "Cuts the corner at the vertex you click with a straight bevel: the first distance back \
         along the segment marked d1, the second along the one marked d2.",
        Script {
            steps: vec![
                vertex_step(click_the("corner vertex", "to cut")),
                value_step(along("first", true), remembered(|kept| kept.chamfer_first)),
                value_step(
                    along("second", false),
                    remembered(|kept| kept.chamfer_second),
                ),
            ],
            plan: Some(plan_chamfer),
            answer: Some(one_vertex),
            remember: Some(remember_chamfer),
            restarts: true,
            ..Script::default()
        },
    )));

    report(catalog.add(info(
        "draw.vertex.merge",
        "Merge Near Vertices",
        170,
        &["MERGEVERTICES"],
        "Merges consecutive vertices closer together than a tolerance into the first of them.",
        selection_script(
            vec![
                select_polylines("merge the near vertices of"),
                value_step(
                    fixed_prompt("Specify the tolerance"),
                    remembered(|kept| kept.merge_tolerance),
                ),
            ],
            Box::new(|c: &Collected| {
                let Some(tolerance) = parse_non_negative(&c.values[0]) else {
                    return ToolStep::rejected(format!("'{}' is not a tolerance", c.values[0]));
                };
                keep_defaults(|kept| kept.merge_tolerance = tolerance);
                ToolStep::done(
                    Some(edit_polylines(
                        c.selection.clone(),
                        "MERGE_VERTICES",
                        move |polyline: &CurvePolyline2| merge_near_vertices(polyline, tolerance),
                    )),
                    format!(
                        "{} merged within {}",
                        plural(c.selection.len(), "polyline"),
                        number(tolerance)
                    ),
                    false,
                )
            }),
        ),
    )));

    report(catalog.add(info(
        "draw.vertex.grid",
        "Snap Vertices to Grid",
        180,
        &["SNAPVERTICES"],
        "Moves every vertex of the selected polylines to the nearest node of a grid.",
        selection_script(
            vec![
                select_polylines("snap to the grid"),
                value_step(
                    fixed_prompt("Specify the grid spacing"),
                    remembered(|kept| kept.grid_spacing),
                ),
            ],
            Box::new(|c: &Collected| {
                let Some(spacing) = parse_positive(&c.values[0]) else {
                    return ToolStep::rejected(format!(
                        "'{}' is not a spacing greater than zero",
                        c.values[0]
                    ));
                };
                keep_defaults(|kept| kept.grid_spacing = spacing);
                ToolStep::done(
                    Some(edit_polylines(
                        c.selection.clone(),
                        "SNAP_VERTICES",
                        move |polyline: &CurvePolyline2| {
                            snap_to_grid(polyline, spacing, Point2::default())
                        },
                    )),
                    format!(
                        "{} snapped to {}",
                        plural(c.selection.len(), "polyline"),
                        number(spacing)
                    ),
                    false,
                )
            }),
        ),
    )));
}
