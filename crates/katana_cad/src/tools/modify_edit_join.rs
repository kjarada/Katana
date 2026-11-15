//! Join and Explode (src/katana_cad/tools/modify_edit_join.cpp), the two
//! halves of one idea: Join chains lines and open polylines that meet end to
//! end into one polyline, Explode takes a polyline (a rectangle is one) apart
//! into its lines. Each acts on a selection - the one the tool was started
//! with, or one made when it asks - and is ONE command.
//!
//! Join carries LINES only. A Katana polyline (geometry's Polyline2) has
//! straight segments and nothing else - no bulges - so an arc cannot become
//! part of one without being replaced by chords, which would change the
//! drawing rather than join it. Arcs in the selection are left as they are
//! and the tool says so.

use katana_entity::entity::{heights_of, set_heights};
use katana_entity::{Entity, EntityId, Geometry};
use katana_geometry::{CurvePolyline2, CurveSegment, Point2, Polyline2};
use katana_math::cpp;
use katana_math::numerics::tolerance;

use crate::document::Document;
use crate::drawing::vertex_editing::read_polyline;
use crate::interactive_tool::{InteractiveTool, ToolContext, ToolInput, ToolStart, ToolStep};
use crate::tool_feedback::ToolFeedback;
use crate::tools::modify_edit_support::{
    Defaults, EditSession, ToolPtr, base_entity, format_number, is_option, no_drawing,
    parse_number, refusal_to_edit, selection_now,
};

type Heights = Vec<Option<f64>>;

/// One line or open polyline, as a run of vertices that can be turned round.
#[derive(Clone)]
struct Run {
    id: EntityId,
    vertices: Vec<Point2>,
    heights: Heights,
}

impl Run {
    fn reverse(&mut self) {
        self.vertices.reverse();
        self.heights.reverse();
    }
}

#[derive(Default)]
struct Chain {
    /// The runs joined; the first is the one kept.
    ids: Vec<EntityId>,
    polyline: Polyline2,
    heights: Heights,
}

#[derive(Default)]
struct JoinPlan {
    /// Two or more runs each.
    chains: Vec<Chain>,
    /// Left out: a polyline cannot carry them.
    arcs: usize,
    /// Left out: circles, closed polylines, points, text.
    other: usize,
    /// Left out: on a locked layer.
    locked: usize,
    /// Lines and open polylines considered.
    runs: usize,
}

fn plural(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

// Appends `run` to the end of the chain (its first vertex is the chain's
// joint), or puts it before the start (its last vertex is the joint). The
// vertex already in the chain is the one kept, so the first run's geometry
// never moves and a later run's end moves by at most the tolerance; its
// height is kept too, unless it has none and the run's has.
fn attach(chain: &mut Chain, run: Run, at_end: bool) {
    let v = &mut chain.polyline.vertices;
    let h = &mut chain.heights;
    if at_end {
        if let Some(last) = h.last_mut()
            && last.is_none()
        {
            *last = run.heights.first().copied().flatten();
        }
        v.extend_from_slice(&run.vertices[1..]);
        h.extend_from_slice(&run.heights[1..]);
    } else {
        if let Some(first) = h.first_mut()
            && first.is_none()
        {
            *first = run.heights.last().copied().flatten();
        }
        let mut vertices = run.vertices[..run.vertices.len() - 1].to_vec();
        vertices.extend_from_slice(v);
        *v = vertices;
        let mut heights = run.heights[..run.heights.len() - 1].to_vec();
        heights.extend_from_slice(h);
        *h = heights;
    }
    chain.ids.push(run.id);
}

fn plan_join(document: &Document, ids: &[EntityId], tolerance: f64) -> JoinPlan {
    let mut plan = JoinPlan::default();
    let mut runs: Vec<Run> = Vec::new();
    for &id in ids {
        let Some(entity) = document.model().entities.find(id) else {
            continue;
        };
        let vertices = match &entity.geometry {
            Geometry::Arc(_) => {
                plan.arcs += 1;
                continue;
            }
            Geometry::Line(segment) => vec![segment.start, segment.end],
            Geometry::Polyline(polyline) if !polyline.closed => polyline.vertices.clone(),
            _ => {
                plan.other += 1;
                continue;
            }
        };
        if refusal_to_edit(document, id).is_some() {
            plan.locked += 1;
            continue;
        }
        let heights = heights_of(&entity.properties, vertices.len());
        runs.push(Run {
            id,
            vertices,
            heights,
        });
    }
    plan.runs = runs.len();

    // Chains grow from the lowest id, first at their end and then at their
    // start, taking the nearest free end within the tolerance each time (the
    // lower id on a tie), so the result is the same on every run.
    let mut used = vec![false; runs.len()];
    for seed in 0..runs.len() {
        if used[seed] {
            continue;
        }
        used[seed] = true;
        let mut chain = Chain {
            ids: vec![runs[seed].id],
            polyline: Polyline2::new(runs[seed].vertices.clone(), false),
            heights: runs[seed].heights.clone(),
        };
        for at_end in [true, false] {
            loop {
                let vertices = &chain.polyline.vertices;
                let joint = if at_end {
                    vertices[vertices.len() - 1]
                } else {
                    vertices[0]
                };
                let mut best: Option<usize> = None;
                let mut best_reversed = false;
                let mut best_distance = f64::INFINITY;
                for (i, run) in runs.iter().enumerate() {
                    if used[i] {
                        continue;
                    }
                    // At the end the run must start at the joint; at the
                    // start it must finish there. Either way round will do.
                    let front = run.vertices[0];
                    let back = run.vertices[run.vertices.len() - 1];
                    let (meets, turned) = if at_end { (front, back) } else { (back, front) };
                    for (end, reversed) in [(meets, false), (turned, true)] {
                        let d = end.distance_to(joint);
                        if d <= tolerance && d < best_distance {
                            best_distance = d;
                            best = Some(i);
                            best_reversed = reversed;
                        }
                    }
                }
                let Some(best) = best else {
                    break;
                };
                used[best] = true;
                let mut run = runs[best].clone();
                if best_reversed {
                    run.reverse();
                }
                attach(&mut chain, run, at_end);
            }
        }
        if chain.ids.len() < 2 {
            continue;
        }
        // A chain whose two ends meet is a closed polyline - when it has the
        // three corners a closed shape needs.
        let v = &chain.polyline.vertices;
        if v.len() >= 4 && v[0].distance_to(v[v.len() - 1]) <= tolerance {
            let last = chain.heights.pop().flatten();
            if let Some(first) = chain.heights.first_mut()
                && first.is_none()
            {
                *first = last;
            }
            chain.polyline.vertices.pop();
            chain.polyline.closed = true;
        }
        plan.chains.push(chain);
    }
    plan
}

// What was left out, as a sentence to add to the report, or nothing.
fn left_out(plan: &JoinPlan, lone: usize) -> String {
    let mut out = String::new();
    if plan.arcs > 0 {
        out.push_str(&format!(
            " {} left out: a polyline has straight segments only.",
            plural(plan.arcs, "arc was", "arcs were")
        ));
    }
    if plan.locked > 0 {
        out.push_str(&format!(
            " {} left out.",
            plural(
                plan.locked,
                "object on a locked layer was",
                "objects on locked layers were"
            )
        ));
    }
    if plan.other > 0 {
        out.push_str(&format!(
            " {} left out.",
            plural(
                plan.other,
                "object that is not a line or open polyline was",
                "objects that are not lines or open polylines were"
            )
        ));
    }
    if lone > 0 {
        out.push_str(&format!(
            " {} left as it was.",
            plural(
                lone,
                "line or polyline met nothing and was",
                "lines or polylines met nothing and were"
            )
        ));
    }
    out
}

// ---- Join --------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum JoinStep {
    Select,
    Tolerance,
}

struct JoinTool {
    defaults: Defaults,
    /// The selection the tool started with (ToolStart).
    selection: Vec<EntityId>,
    session: EditSession,
    step: JoinStep,
    picked: Vec<EntityId>,
}

impl JoinTool {
    // A tolerance of zero still has to forgive the last bit of a computed
    // end.
    fn tolerance(&self) -> f64 {
        cpp::max(self.defaults.borrow().join_tolerance, tolerance::GEOMETRIC)
    }
}

impl InteractiveTool for JoinTool {
    fn prompt(&self, cx: &ToolContext<'_>) -> String {
        let remembered = self.defaults.borrow().join_tolerance;
        if self.step == JoinStep::Tolerance {
            return format!(
                "Specify how far apart two ends may be and still join <{}>",
                format_number(remembered)
            );
        }
        let count = selection_now(cx, &self.selection, &self.picked).len();
        format!(
            "Select lines and polylines to join or [Tolerance] ({}, ends within {})",
            plural(count, "selected", "selected"),
            format_number(remembered)
        )
    }

    fn expects(&self, _cx: &ToolContext<'_>) -> ToolInput {
        if self.step == JoinStep::Tolerance {
            ToolInput::Value
        } else {
            ToolInput::Selection
        }
    }

    fn entity(&mut self, cx: &ToolContext<'_>, id: EntityId, at: Point2) -> ToolStep {
        if self.step != JoinStep::Select {
            return base_entity(cx, self.expects(cx), id, at);
        }
        if !self.picked.contains(&id) {
            self.picked.push(id);
        }
        ToolStep::next("")
    }

    fn value(&mut self, cx: &ToolContext<'_>, text: &str) -> ToolStep {
        if self.step == JoinStep::Tolerance {
            let Some(tolerance) = parse_number(text) else {
                return ToolStep::rejected("Type the tolerance as a distance.");
            };
            if tolerance < 0.0 {
                return ToolStep::rejected("The tolerance cannot be negative.");
            }
            self.defaults.borrow_mut().join_tolerance = tolerance;
            self.step = JoinStep::Select;
            return ToolStep::next("");
        }
        if is_option(text, "Tolerance", "T") {
            self.step = JoinStep::Tolerance;
            return ToolStep::next("");
        }
        if is_option(text, "Undo", "U") {
            return self.undo(cx);
        }
        ToolStep::rejected(format!("'{text}' is not an option here."))
    }

    fn enter(&mut self, cx: &ToolContext<'_>) -> ToolStep {
        if self.step == JoinStep::Tolerance {
            self.step = JoinStep::Select;
            return ToolStep::next("");
        }
        let Some(document) = cx.document else {
            return no_drawing();
        };
        let ids = selection_now(cx, &self.selection, &self.picked);
        let plan = plan_join(document, &ids, self.tolerance());
        if plan.chains.is_empty() {
            if plan.runs < 2 {
                return ToolStep::rejected(format!(
                    "Select at least two lines or open polylines to join.{}",
                    left_out(&plan, 0)
                ));
            }
            return ToolStep::rejected(format!(
                "None of the selected lines and polylines meet end to end within {}.{}",
                format_number(self.tolerance()),
                left_out(&plan, 0)
            ));
        }
        let mut joined = 0;
        self.session.begin();
        for chain in &plan.chains {
            // The lowest id is kept, as AutoCAD keeps the source object: its
            // layer, style, colour and properties are the joined polyline's.
            let Some(&keep) = chain.ids.iter().min() else {
                continue;
            };
            let Some(original) = self.session.original(document, keep) else {
                continue;
            };
            let mut kept = original.clone();
            kept.geometry = Geometry::Polyline(chain.polyline.clone());
            set_heights(&mut kept.properties, &chain.heights);
            if let Err(error) = self.session.replace(keep, vec![kept]) {
                return ToolStep::rejected(error.message);
            }
            for &id in &chain.ids {
                // Removing an entity adds no piece, so this cannot fail.
                if id != keep {
                    let _ = self.session.replace(id, Vec::new());
                }
            }
            joined += chain.ids.len();
        }
        let lone = plan.runs - joined;
        ToolStep::done(
            Some(self.session.commit("JOIN")),
            format!(
                "Joined {} into {}.{}",
                plural(joined, "object", "objects"),
                plural(plan.chains.len(), "polyline", "polylines"),
                left_out(&plan, lone)
            ),
            false,
        )
    }

    fn undo(&mut self, _cx: &ToolContext<'_>) -> ToolStep {
        if self.step == JoinStep::Tolerance {
            self.step = JoinStep::Select;
            return ToolStep::next("");
        }
        if self.picked.pop().is_none() {
            return ToolStep::rejected("Nothing to undo.");
        }
        ToolStep::next("")
    }

    fn preview(&self, cx: &ToolContext<'_>, _cursor: Point2) -> ToolFeedback {
        // The polylines Enter would make from what is selected now.
        let mut feedback = ToolFeedback::default();
        let Some(document) = cx.document else {
            return feedback;
        };
        let plan = plan_join(
            document,
            &selection_now(cx, &self.selection, &self.picked),
            self.tolerance(),
        );
        for chain in plan.chains {
            feedback.shapes.push(Geometry::Polyline(chain.polyline));
        }
        feedback
    }
}

// ---- Explode -----------------------------------------------------------------------

// A line per straight segment and an arc per arc segment - the closing one
// too - wearing the polyline's layer, style, colour and properties, with the
// heights of its two ends (an arc's in its start-to-end order, as the model
// keeps an arc's). A segment with no length (a repeated vertex) makes
// nothing: the model would refuse it.
fn explode(entity: &Entity, polyline: &CurvePolyline2) -> Vec<Entity> {
    let mut pieces = Vec::new();
    for i in 0..polyline.segment_count() {
        let geometry = match polyline.segment(i) {
            CurveSegment::Segment(segment) => {
                if segment.is_degenerate() {
                    continue;
                }
                Geometry::Line(segment)
            }
            CurveSegment::Arc(arc) => Geometry::Arc(arc),
        };
        let mut made = entity.clone();
        made.geometry = geometry;
        set_heights(
            &mut made.properties,
            &[
                polyline.vertices[i].height,
                polyline.vertices[polyline.segment_end(i)].height,
            ],
        );
        pieces.push(made);
    }
    pieces
}

struct ExplodeTool {
    /// The selection the tool started with (ToolStart).
    selection: Vec<EntityId>,
    session: EditSession,
    picked: Vec<EntityId>,
}

impl InteractiveTool for ExplodeTool {
    fn prompt(&self, cx: &ToolContext<'_>) -> String {
        let count = selection_now(cx, &self.selection, &self.picked).len();
        format!(
            "Select polylines to explode ({})",
            plural(count, "selected", "selected")
        )
    }

    fn expects(&self, _cx: &ToolContext<'_>) -> ToolInput {
        ToolInput::Selection
    }

    fn entity(&mut self, _cx: &ToolContext<'_>, id: EntityId, _at: Point2) -> ToolStep {
        if !self.picked.contains(&id) {
            self.picked.push(id);
        }
        ToolStep::next("")
    }

    fn value(&mut self, cx: &ToolContext<'_>, text: &str) -> ToolStep {
        if is_option(text, "Undo", "U") {
            return self.undo(cx);
        }
        ToolStep::rejected("Select the polylines to explode and press Enter.")
    }

    fn enter(&mut self, cx: &ToolContext<'_>) -> ToolStep {
        let ids = selection_now(cx, &self.selection, &self.picked);
        if ids.is_empty() {
            return ToolStep::rejected("Select the polylines to explode first.");
        }
        let Some(document) = cx.document else {
            return no_drawing();
        };
        let mut polylines = 0;
        let mut lines = 0;
        let mut locked = 0;
        let mut other = 0;
        let mut exploded: Vec<(EntityId, Vec<Entity>)> = Vec::new();
        for &id in &ids {
            let Some(entity) = self.session.original(document, id) else {
                continue;
            };
            // Either polyline kind (docs/drawing.md): a curve polyline's arc
            // segments come apart as arcs.
            let Some(polyline) = read_polyline(entity) else {
                other += 1;
                continue;
            };
            if refusal_to_edit(document, id).is_some() {
                locked += 1;
                continue;
            }
            let pieces = explode(entity, &polyline);
            if pieces.is_empty() {
                other += 1; // every vertex in one place: there is no line to make
                continue;
            }
            polylines += 1;
            lines += pieces.len();
            exploded.push((id, pieces));
        }
        let mut note = String::new();
        if locked > 0 {
            note.push_str(&format!(
                " {} left as it was.",
                plural(
                    locked,
                    "polyline on a locked layer was",
                    "polylines on locked layers were"
                )
            ));
        }
        if other > 0 {
            note.push_str(&format!(
                " {} left as it was.",
                plural(
                    other,
                    "object that is not a polyline was",
                    "objects that are not polylines were"
                )
            ));
        }
        if exploded.is_empty() {
            return ToolStep::rejected(format!(
                "Only polylines can be exploded, and the selection has none that can be.{note}"
            ));
        }
        self.session.begin();
        for (id, pieces) in exploded {
            let _ = self.session.replace(id, Vec::new()); // a removal adds no piece: it cannot fail
            for piece in pieces {
                if let Err(error) = self.session.add(piece) {
                    return ToolStep::rejected(error.message);
                }
            }
        }
        ToolStep::done(
            Some(self.session.commit("EXPLODE")),
            format!(
                "Exploded {} into {}.{note}",
                plural(polylines, "polyline", "polylines"),
                plural(lines, "line", "lines")
            ),
            false,
        )
    }

    fn undo(&mut self, _cx: &ToolContext<'_>) -> ToolStep {
        if self.picked.pop().is_none() {
            return ToolStep::rejected("Nothing to undo.");
        }
        ToolStep::next("")
    }

    fn preview(&self, cx: &ToolContext<'_>, _cursor: Point2) -> ToolFeedback {
        // Where the polylines will come apart: every vertex of each.
        let mut feedback = ToolFeedback::default();
        let Some(document) = cx.document else {
            return feedback;
        };
        for id in selection_now(cx, &self.selection, &self.picked) {
            if let Some(polyline) = self.session.original(document, id).and_then(read_polyline) {
                feedback.markers.extend(polyline.positions());
            }
        }
        feedback
    }
}

pub fn make_join_tool(start: &ToolStart, defaults: Defaults) -> ToolPtr {
    Box::new(JoinTool {
        defaults,
        selection: start.selection.clone(),
        session: EditSession::new(),
        step: JoinStep::Select,
        picked: Vec::new(),
    })
}

pub fn make_explode_tool(start: &ToolStart) -> ToolPtr {
    Box::new(ExplodeTool {
        selection: start.selection.clone(),
        session: EditSession::new(),
        picked: Vec::new(),
    })
}
