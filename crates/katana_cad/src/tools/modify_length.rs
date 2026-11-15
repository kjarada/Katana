//! Lengthen and Reverse (src/katana_cad/tools/modify_length.cpp; see
//! families.rs).
//!
//! Lengthen follows AutoCAD's LENGTHEN dialogue: pick an object to be told
//! its length, or choose how to change one - DElta (by so much; negative
//! shortens), Percent (to so much of what it is), Total (to so long) or
//! DYnamic (to where the cursor is) - then pick objects near the end to move.
//! Lines, arcs and open polylines have ends; a closed outline or a circle has
//! none and is refused. A polyline grows along its end segment and shrinks
//! back along its path, dropping the vertices it passes; an arc keeps its
//! centre and radius and changes its sweep. Every pick is one step of one
//! session (modify_edit_support's EditSession), so one undo takes back the
//! lot, and Esc keeps what was already changed.
//!
//! Reverse turns lines, arcs and polylines round - the same geometry, drawn
//! from the other end - with the heights of a 3D string turned round with
//! them. What has a direction matters wherever order does: a linestyle's
//! ticks, a chainage, a string's heights, an export.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use katana_commands::{ChangeSet, ChangeSetCommand};
use katana_core::{Error, ErrorCode, Result};
use katana_entity::entity::{INVALID_ENTITY_ID, heights_of, set_heights};
use katana_entity::{Entity, EntityId, Geometry};
use katana_geometry::{Arc2, Point2, Segment2};
use katana_math::numerics::{TWO_PI, tolerance};

use crate::interactive_tool::{
    InteractiveTool, ToolCatalog, ToolContext, ToolFactory, ToolInfo, ToolInput, ToolStart,
    ToolStep, ToolStepOutcome,
};
use crate::tool_feedback::ToolFeedback;
use crate::tools::everyday_support::{Path, SelectionStep, counted, degrees, fixed};
use crate::tools::families::Report;
use crate::tools::modify_edit_support::{
    EditSession, PolylinePath, ToolPtr, as_sentence, base_entity, base_point, carry_heights,
    format_number, is_option, kind_name, no_drawing, parse_number, refusal_to_edit, tool_factory,
    with_article,
};

// ---- Lengthen ----------------------------------------------------------------------

/// What Lengthen remembers from one use to the next, as AutoCAD remembers
/// the last delta, percentage and total: one object per program, shared by
/// the tool's factory; nothing is stored in the drawing.
#[derive(Clone, Debug, PartialEq)]
struct LengthenDefaults {
    delta: f64,
    percent: f64,
    total: f64,
}

impl Default for LengthenDefaults {
    fn default() -> Self {
        Self {
            delta: 0.0,
            percent: 100.0,
            total: 1.0,
        }
    }
}

thread_local! {
    // The program's one set (the C++ shared_ptr the factory captures); the
    // program runs its tools on one thread, so the set is that thread's
    // (cad_plan.md section 7, risk 4).
    static LENGTHEN_DEFAULTS: Rc<RefCell<LengthenDefaults>> =
        Rc::new(RefCell::new(LengthenDefaults::default()));
}

// Why `geometry` has no end to lengthen, or None when it has.
fn without_ends(geometry: &Geometry) -> Option<String> {
    match geometry {
        Geometry::Line(_) | Geometry::Arc(_) => None,
        Geometry::Polyline(polyline) => {
            if polyline.closed {
                Some(String::from("a closed polyline has no end to lengthen"))
            } else {
                None
            }
        }
        _ => Some(format!(
            "{} has no end to lengthen",
            with_article(kind_name(geometry))
        )),
    }
}

// True when the end of `geometry` nearer to `at` is its start.
fn start_is_nearer(geometry: &Geometry, at: Point2) -> bool {
    match geometry {
        Geometry::Line(line) => at.distance_to(line.start) < at.distance_to(line.end),
        Geometry::Arc(arc) => at.distance_to(arc.start_point()) < at.distance_to(arc.end_point()),
        // Along the polyline, not as the crow flies: a pick on the first leg
        // of a hairpin is nearer the start whatever the straight distances
        // say.
        Geometry::Polyline(polyline) => {
            let path = PolylinePath::new(polyline.clone());
            path.station_of(at) < 0.5 * path.length()
        }
        // without_ends refuses every other kind first.
        _ => false,
    }
}

// `geometry` with the end at its start (at_start) or its end moved so that
// it is `length` long, the other end staying where it is.
fn lengthened(geometry: &Geometry, at_start: bool, length: f64) -> Result<Geometry> {
    if !length.is_finite() || length <= tolerance::GEOMETRIC {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the new length must be more than zero",
        ));
    }
    match geometry {
        Geometry::Line(line) => {
            let fixed_end = if at_start { line.end } else { line.start };
            let moving = if at_start { line.start } else { line.end };
            let along = (moving - fixed_end).normalized();
            let moved = fixed_end + along * length;
            Ok(Geometry::Line(if at_start {
                Segment2::new(moved, line.end)
            } else {
                Segment2::new(line.start, moved)
            }))
        }
        Geometry::Arc(arc) => {
            let sweep = (length / arc.radius).copysign(arc.sweep);
            if sweep.abs() >= TWO_PI {
                return Err(Error::new(
                    ErrorCode::InvalidArgument,
                    "an arc that long would go all the way round its circle",
                ));
            }
            Ok(Geometry::Arc(if at_start {
                Arc2::new(arc.center, arc.radius, arc.end_angle() - sweep, sweep)
            } else {
                Arc2::new(arc.center, arc.radius, arc.start_angle, sweep)
            }))
        }
        Geometry::Polyline(polyline) => {
            let path = PolylinePath::new(polyline.clone());
            let current = path.length();
            if length < current {
                return Ok(Geometry::Polyline(if at_start {
                    path.between(current - length, current)
                } else {
                    path.between(0.0, length)
                }));
            }
            // Longer: the end segment goes on in its own direction.
            let mut longer = polyline.clone();
            let v = &mut longer.vertices;
            let end = if at_start { 0 } else { v.len() - 1 };
            let inner = if at_start { 1 } else { v.len() - 2 };
            let outward = v[end] - v[inner];
            if outward.length() <= tolerance::GEOMETRIC {
                return Err(Error::new(
                    ErrorCode::InvalidGeometry,
                    "the polyline's end segment has no length, so no direction to go on in",
                ));
            }
            v[end] += outward.normalized() * (length - current);
            Ok(Geometry::Polyline(longer))
        }
        // without_ends refuses every other kind first.
        _ => Err(Error::new(
            ErrorCode::Unsupported,
            format!(
                "{} has no end to lengthen",
                with_article(kind_name(geometry))
            ),
        )),
    }
}

// The length that puts the moving end where `cursor` says (DYnamic): along a
// line's own direction, round an arc's circle, along a polyline's path - or
// on along its end segment when the cursor is beyond the end.
fn length_to(geometry: &Geometry, at_start: bool, cursor: Point2) -> f64 {
    match geometry {
        Geometry::Line(line) => {
            let fixed_end = if at_start { line.end } else { line.start };
            let moving = if at_start { line.start } else { line.end };
            (cursor - fixed_end).dot((moving - fixed_end).normalized())
        }
        Geometry::Arc(arc) => {
            let direction = (cursor.y - arc.center.y).atan2(cursor.x - arc.center.x);
            let sign = if arc.sweep >= 0.0 { 1.0 } else { -1.0 };
            // Turned from the fixed end in the arc's own sense, in [0, 2*pi).
            let from = if at_start {
                arc.end_angle()
            } else {
                arc.start_angle
            };
            let mut turned = if at_start {
                sign * (from - direction)
            } else {
                sign * (direction - from)
            };
            // std::fmod: the remainder takes the sign of the dividend, as
            // Rust's % on reals does.
            turned %= TWO_PI;
            if turned < 0.0 {
                turned += TWO_PI;
            }
            arc.radius * turned
        }
        Geometry::Polyline(polyline) => {
            let path = PolylinePath::new(polyline.clone());
            let v = &polyline.vertices;
            let end = if at_start { v[0] } else { v[v.len() - 1] };
            let outward = if at_start {
                v[0] - v[1]
            } else {
                v[v.len() - 1] - v[v.len() - 2]
            };
            if outward.length() > tolerance::GEOMETRIC {
                let beyond = (cursor - end).dot(outward.normalized());
                if beyond > 0.0 {
                    return path.length() + beyond;
                }
            }
            let station = path.station_of(cursor);
            if at_start {
                path.length() - station
            } else {
                station
            }
        }
        _ => 0.0,
    }
}

fn length_of(geometry: &Geometry) -> f64 {
    Path::of(geometry).map_or(0.0, |path| path.length())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Delta,
    Percent,
    Total,
    Dynamic,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Measure,
    Amount,
    Change,
    DynamicEnd,
}

struct LengthenTool {
    session: EditSession,
    defaults: Rc<RefCell<LengthenDefaults>>,
    mode: Mode,
    step: Step,
    target: EntityId,
    target_at_start: bool,
}

impl LengthenTool {
    fn choose_mode(&mut self, mode: Mode) -> ToolStep {
        self.mode = mode;
        self.step = if mode == Mode::Dynamic {
            Step::Change
        } else {
            Step::Amount
        };
        ToolStep::next("")
    }

    fn choose_amount(&mut self, number: f64) -> ToolStep {
        match self.mode {
            Mode::Delta => self.defaults.borrow_mut().delta = number,
            Mode::Percent => {
                if number <= 0.0 {
                    return ToolStep::rejected("a percentage length must be more than zero");
                }
                self.defaults.borrow_mut().percent = number;
            }
            Mode::Total | Mode::Dynamic => {
                if number <= 0.0 {
                    return ToolStep::rejected("a total length must be more than zero");
                }
                self.defaults.borrow_mut().total = number;
            }
        }
        self.step = Step::Change;
        ToolStep::next("")
    }

    fn change(&mut self, id: EntityId, current: &Entity, at_start: bool, length: f64) -> ToolStep {
        let geometry = match lengthened(&current.geometry, at_start, length) {
            Ok(geometry) => geometry,
            Err(error) => return ToolStep::rejected(as_sentence(&error.message)),
        };
        let mut piece = current.clone();
        piece.geometry = geometry;
        carry_heights(current, &mut piece);
        self.session.begin();
        if let Err(error) = self.session.replace(id, vec![piece]) {
            return ToolStep::rejected(as_sentence(&error.message));
        }
        ToolStep::next(format!("Length {}", fixed(length, 3)))
    }

    fn finish(&self) -> ToolStep {
        let count = self.session.edited_ids().len();
        if self.session.operations() == 0 {
            return ToolStep::done(None, "", false);
        }
        ToolStep::done(
            Some(self.session.commit("LENGTHEN")),
            format!("Lengthened {}", counted(count, "object", "objects")),
            false,
        )
    }
}

impl InteractiveTool for LengthenTool {
    fn prompt(&self, _cx: &ToolContext<'_>) -> String {
        let defaults = self.defaults.borrow();
        match self.step {
            Step::Measure => {
                String::from("Select an object to measure or [DElta/Percent/Total/DYnamic]")
            }
            Step::Amount => match self.mode {
                Mode::Delta => format!("Enter delta length <{}>", format_number(defaults.delta)),
                Mode::Percent => format!(
                    "Enter percentage length <{}>",
                    format_number(defaults.percent)
                ),
                Mode::Total | Mode::Dynamic => {
                    format!("Specify total length <{}>", format_number(defaults.total))
                }
            },
            Step::Change => String::from("Select an object to change or [Undo], Enter to finish"),
            Step::DynamicEnd => String::from("Specify new end point"),
        }
    }

    fn expects(&self, _cx: &ToolContext<'_>) -> ToolInput {
        match self.step {
            Step::Measure | Step::Change => ToolInput::Entity,
            Step::Amount => ToolInput::Value,
            Step::DynamicEnd => ToolInput::Point,
        }
    }

    fn entity(&mut self, cx: &ToolContext<'_>, id: EntityId, at: Point2) -> ToolStep {
        if self.step != Step::Measure && self.step != Step::Change {
            return base_entity(cx, self.expects(cx), id, at);
        }
        let Some(document) = cx.document else {
            return no_drawing();
        };
        if self.step == Step::Change
            && let Some(refusal) = refusal_to_edit(document, id)
        {
            return ToolStep::rejected(refusal);
        }
        let pieces = self.session.pieces(document, id);
        let [current] = pieces.as_slice() else {
            return ToolStep::rejected("that entity is not in the drawing");
        };
        if self.step == Step::Measure {
            // Measuring asks nothing of the object's ends: a circle has a
            // length too.
            let mut text = format!("Current length {}", fixed(length_of(&current.geometry), 3));
            if let Geometry::Arc(arc) = &current.geometry {
                text.push_str(&format!(", included angle {}", degrees(arc.sweep.abs())));
            }
            return ToolStep::next(text);
        }
        if let Some(why) = without_ends(&current.geometry) {
            return ToolStep::rejected(why);
        }
        let at_start = start_is_nearer(&current.geometry, at);
        if self.mode == Mode::Dynamic {
            self.target = id;
            self.target_at_start = at_start;
            self.step = Step::DynamicEnd;
            return ToolStep::next("");
        }
        let now = length_of(&current.geometry);
        let defaults = self.defaults.borrow().clone();
        let length = match self.mode {
            Mode::Delta => {
                if defaults.delta == 0.0 {
                    return ToolStep::rejected(
                        "a delta of zero changes nothing; type DE to set one",
                    );
                }
                now + defaults.delta
            }
            Mode::Percent => now * defaults.percent / 100.0,
            Mode::Total => defaults.total,
            Mode::Dynamic => now,
        };
        self.change(id, current, at_start, length)
    }

    fn point(&mut self, cx: &ToolContext<'_>, at: Point2) -> ToolStep {
        if self.step != Step::DynamicEnd {
            return base_point(cx, self.expects(cx), at);
        }
        let Some(document) = cx.document else {
            return no_drawing();
        };
        let pieces = self.session.pieces(document, self.target);
        let [current] = pieces.as_slice() else {
            self.step = Step::Change;
            return ToolStep::rejected("that entity is not in the drawing");
        };
        let length = length_to(&current.geometry, self.target_at_start, at);
        let step = self.change(self.target, current, self.target_at_start, length);
        if step.outcome != ToolStepOutcome::Rejected {
            self.step = Step::Change;
        }
        step
    }

    fn value(&mut self, cx: &ToolContext<'_>, text: &str) -> ToolStep {
        if self.step == Step::Amount {
            let Some(number) = parse_number(text) else {
                return ToolStep::rejected("type a number, or press Enter for the default");
            };
            return self.choose_amount(number);
        }
        if self.step == Step::Measure || self.step == Step::Change {
            if self.step == Step::Change && is_option(text, "Undo", "U") {
                return self.undo(cx);
            }
            if is_option(text, "DElta", "DE") {
                return self.choose_mode(Mode::Delta);
            }
            if is_option(text, "Percent", "P") {
                return self.choose_mode(Mode::Percent);
            }
            if is_option(text, "Total", "T") {
                return self.choose_mode(Mode::Total);
            }
            if is_option(text, "DYnamic", "DY") {
                return self.choose_mode(Mode::Dynamic);
            }
        }
        ToolStep::rejected(format!("'{text}' is not an option here"))
    }

    fn enter(&mut self, _cx: &ToolContext<'_>) -> ToolStep {
        match self.step {
            Step::Amount => {
                // The default shown in the prompt.
                let defaults = self.defaults.borrow().clone();
                self.choose_amount(match self.mode {
                    Mode::Delta => defaults.delta,
                    Mode::Percent => defaults.percent,
                    Mode::Total | Mode::Dynamic => defaults.total,
                })
            }
            Step::DynamicEnd => ToolStep::rejected("click or type the new end point"),
            Step::Measure | Step::Change => self.finish(),
        }
    }

    fn undo(&mut self, _cx: &ToolContext<'_>) -> ToolStep {
        match self.step {
            Step::DynamicEnd => {
                self.step = Step::Change;
                ToolStep::next("")
            }
            Step::Change => {
                if self.session.undo() {
                    return ToolStep::next("Undid the last change.");
                }
                self.step = Step::Measure;
                ToolStep::next("")
            }
            Step::Amount => {
                self.step = Step::Measure;
                ToolStep::next("")
            }
            Step::Measure => ToolStep::rejected("nothing to undo in this tool"),
        }
    }

    // Esc keeps every change already made; it never applies a default.
    fn cancel(&mut self, _cx: &ToolContext<'_>) -> ToolStep {
        if self.session.operations() == 0 {
            return ToolStep::done(None, "", false);
        }
        self.finish()
    }

    fn preview(&self, cx: &ToolContext<'_>, cursor: Point2) -> ToolFeedback {
        let mut feedback = ToolFeedback {
            shapes: self.session.shapes(),
            ..ToolFeedback::default()
        };
        if self.step == Step::DynamicEnd
            && let Some(document) = cx.document
        {
            let pieces = self.session.pieces(document, self.target);
            if let [current] = pieces.as_slice() {
                let length = length_to(&current.geometry, self.target_at_start, cursor);
                if let Ok(shape) = lengthened(&current.geometry, self.target_at_start, length) {
                    feedback.shapes.push(shape);
                }
            }
        }
        feedback
    }
}

// ---- Reverse -----------------------------------------------------------------------

// `entity` drawn from its other end, heights and all; None for a kind with no
// direction.
fn reversed(entity: &Entity) -> Option<Entity> {
    let mut out = entity.clone();
    let count = match &entity.geometry {
        Geometry::Line(line) => {
            out.geometry = Geometry::Line(Segment2::new(line.end, line.start));
            2
        }
        Geometry::Arc(arc) => {
            out.geometry = Geometry::Arc(arc.reversed());
            2
        }
        Geometry::Polyline(polyline) => {
            let mut turned = polyline.clone();
            turned.vertices.reverse();
            let count = turned.vertices.len();
            out.geometry = Geometry::Polyline(turned);
            count
        }
        _ => return None,
    };
    let mut heights = heights_of(&entity.properties, count);
    // Rewritten only when there are heights: an entity with none keeps its
    // properties exactly as they were.
    if heights.iter().any(Option::is_some) {
        heights.reverse();
        set_heights(&mut out.properties, &heights);
    }
    Some(out)
}

#[derive(Default)]
struct ReverseTool {
    selection: SelectionStep,
}

impl InteractiveTool for ReverseTool {
    fn prompt(&self, _cx: &ToolContext<'_>) -> String {
        String::from("Select lines, arcs and polylines to reverse or [All], then press Enter")
    }

    fn expects(&self, _cx: &ToolContext<'_>) -> ToolInput {
        ToolInput::Selection
    }

    fn entity(&mut self, cx: &ToolContext<'_>, id: EntityId, _at: Point2) -> ToolStep {
        self.selection.pick(cx.document, id)
    }

    fn value(&mut self, cx: &ToolContext<'_>, text: &str) -> ToolStep {
        if is_option(text, "All", "A") {
            return self.selection.all(cx.document);
        }
        ToolStep::rejected(format!(
            "'{text}' is not an option here: pick entities, or type All"
        ))
    }

    fn undo(&mut self, _cx: &ToolContext<'_>) -> ToolStep {
        if self.selection.undo() {
            ToolStep::next("")
        } else {
            ToolStep::rejected("nothing to undo in this tool")
        }
    }

    fn enter(&mut self, cx: &ToolContext<'_>) -> ToolStep {
        let Some(document) = cx.document else {
            return ToolStep::rejected("there is no drawing");
        };
        let ids = self.selection.chosen(Some(document));
        if ids.is_empty() {
            return ToolStep::rejected(
                "nothing is selected: select the objects to reverse, then press Enter",
            );
        }
        let mut changes = ChangeSet::default();
        // Why each skipped entity was skipped, counted by reason.
        let mut skipped: BTreeMap<String, usize> = BTreeMap::new();
        for &id in &ids {
            let Some(entity) = document.model().entities.find(id) else {
                continue;
            };
            if refusal_to_edit(document, id).is_some() {
                *skipped
                    .entry(String::from("on a locked layer"))
                    .or_insert(0) += 1;
                continue;
            }
            match reversed(entity) {
                Some(turned) => changes.modify.push(turned),
                None => {
                    *skipped
                        .entry(format!("{} has no direction", kind_name(&entity.geometry)))
                        .or_insert(0) += 1;
                }
            }
        }
        let mut notes = String::new();
        for (reason, count) in &skipped {
            notes.push_str(&format!("; {count} skipped: {reason}"));
        }
        if changes.modify.is_empty() {
            return ToolStep::rejected(format!("nothing selected can be reversed{notes}"));
        }
        let count = changes.modify.len();
        let command = Box::new(ChangeSetCommand::new(
            "REVERSE",
            move |_context| Ok(changes.clone()),
            false,
        ));
        ToolStep::done(
            Some(command),
            format!("Reversed {}{notes}", counted(count, "object", "objects")),
            false,
        )
    }
}

fn lengthen_tool(_cx: &ToolContext<'_>, _start: &ToolStart) -> ToolPtr {
    Box::new(LengthenTool {
        session: EditSession::new(),
        defaults: LENGTHEN_DEFAULTS.with(Rc::clone),
        mode: Mode::Delta,
        step: Step::Measure,
        target: INVALID_ENTITY_ID,
        target_at_start: false,
    })
}

fn reverse_tool(_cx: &ToolContext<'_>, _start: &ToolStart) -> ToolPtr {
    Box::new(ReverseTool::default())
}

pub fn add_modify_length_tools(catalog: &mut ToolCatalog, report: &mut Report<'_>) {
    let mut add =
        |id: &str, name: &str, order: i32, aliases: &[&str], tip: &str, make: ToolFactory| {
            let info = ToolInfo {
                id: String::from(id),
                name: String::from(name),
                category: String::from("Modify"),
                group: String::from("Edit"),
                order,
                aliases: aliases.iter().map(|&alias| String::from(alias)).collect(),
                tip: String::from(tip),
                make: Some(make),
                ..ToolInfo::default()
            };
            report(catalog.add(info));
        };
    add(
        "modify.lengthen",
        "Lengthen",
        100,
        &["LENGTHEN", "LEN"],
        "Changes the length of lines, arcs and open polylines at the end nearer the pick: DE by \
         a delta, P to a percentage, T to a total, DY to the cursor; pick an object first to be \
         told its length.",
        tool_factory(lengthen_tool),
    );
    add(
        "modify.reverse",
        "Reverse",
        110,
        &["REVERSE", "REV"],
        "Turns lines, arcs and polylines round so they run from the other end, with a 3D \
         string's heights turned round too.",
        tool_factory(reverse_tool),
    );
}
