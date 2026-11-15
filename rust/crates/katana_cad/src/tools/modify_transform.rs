//! Move, Copy, Rotate, Scale, Mirror, Stretch, Array (rectangular and polar)
//! and Erase: the tools that act on a selection as a whole (tools/families.rs;
//! src/katana_cad/tools/modify_transform.cpp).
//!
//! Each follows its AutoCAD namesake's dialogue, because that is what a CAD
//! user's hands already know: the same prompts, the same option letters, the
//! same defaults in <angle brackets>, Enter to accept one. Where AutoCAD and
//! the command interpreter's verb of the same name differ, the interpreter
//! wins on MEANING (ARRAY is rectangular there, so it is here) and AutoCAD on
//! DIALOGUE (Mirror keeps the source unless told otherwise).
//!
//! A tool never touches the document. It reads the selection's geometry from
//! it for the rubber band and for a few decisions (what a window catches, the
//! middle of what a polar array turns), and when it completes it returns ONE
//! command, so one undo takes the whole operation back - a Copy to five
//! places included.
//!
//! The C++'s SelectionTool is a base class whose virtual step methods each
//! tool overrides; here it is `SelectionTool` generic over the tool's
//! `TransformSteps`, which are lent the selection (`Chosen`) per call.

use std::cell::Cell;

use katana_commands::{
    ChangeSet, ChangeSetCommand, CommandPtr, array_entities, delete_entities, duplicate_entities,
    mirror_entities, move_entities, rotate_entities, scale_entities, validate_change_set,
};
use katana_core::error::make_error;
use katana_core::text::{equals_ignoring_case, parse_finite_double, parse_integer};
use katana_core::{Error, ErrorCode, Result};
use katana_entity::entity_geometry::{bounding_box, transformed};
use katana_entity::{EntityId, Geometry, Model, PointGeometry, validate};
use katana_geometry::{Arc2, Box2, Circle2, Point2, Polyline2, Segment2, Spline2, Vec2};
use katana_math::Mat3;
use katana_math::numerics::{DEG_TO_RAD, angles_equal, nearly_equal, tolerance};

use crate::document::Document;
use crate::interactive_tool::{
    InteractiveTool, ToolCatalog, ToolContext, ToolFactory, ToolInfo, ToolInput, ToolStart,
    ToolStep,
};
use crate::layer_overrides::NO_LAYER_OVERRIDES;
use crate::selection::{BoxSelectionMode, SelectionFilter, is_selectable, pick_in_box};
use crate::tool_feedback::ToolFeedback;
use crate::tools::draw_lines::{DefaultAnswers, factory};
use crate::tools::everyday_support::counted;
use crate::tools::families::Report;
use crate::utilities::utility_data::trimmed_text;

// The rubber band is redrawn on every mouse move, so it shows at most this many
// shapes; a selection bigger than that is shown by its base point and the band
// alone. A thousand lines is far more than the eye follows while dragging, and
// few enough that transforming them per move costs nothing noticeable.
const PREVIEW_LIMIT: usize = 1000;

// The array command's own guard against a runaway typo (entity_commands).
// The tools refuse the same count up front, with a sentence, rather than
// handing the document a command it will reject.
const MAXIMUM_ARRAY_ITEMS: i64 = 100_000;

// ---- small helpers -----------------------------------------------------------------

// "1 entity", "3 entities".
fn entities(count: usize) -> String {
    counted(count, "entity", "entities")
}

fn number_in(text: &str) -> Option<f64> {
    parse_finite_double(trimmed_text(text))
}

fn integer_in(text: &str) -> Option<i64> {
    parse_integer(trimmed_text(text))
}

// An option as a prompt offers it: its key letters or the whole word, in any
// case - "C" or "copy" for [Copy].
fn is_option(text: &str, key: &str, word: &str) -> bool {
    let input = trimmed_text(text).as_bytes();
    equals_ignoring_case(input, key.as_bytes()) || equals_ignoring_case(input, word.as_bytes())
}

fn coincident(a: Point2, b: Point2) -> bool {
    a.distance_to(b) <= tolerance::GEOMETRIC
}

fn not_understood(text: &str, wanted: &str) -> ToolStep {
    ToolStep::rejected(format!("'{}' is not {wanted}", trimmed_text(text)))
}

// AutoCAD's direct distance entry: a bare number where a point is wanted means
// that far from `from` towards the cursor. The direction is the rubber band's,
// which is why the tools remember the last cursor they previewed.
fn along_cursor(distance: f64, from: Point2, cursor: Option<Point2>) -> Option<Point2> {
    let cursor = cursor.filter(|&cursor| !coincident(cursor, from))?;
    Some(from + (cursor - from).normalized() * distance)
}

const NO_DIRECTION: &str = "a typed distance goes towards the cursor: move the cursor off the base \
                            point first, or type @dx,dy";

// The origin, which "@3,4" typed as a displacement is measured from.
const ORIGIN: Point2 = Point2::new(0.0, 0.0);

// The selection's geometry through each transform, into the rubber band, up to
// the preview limit.
fn add_transformed(
    feedback: &mut ToolFeedback,
    document: Option<&Document>,
    ids: &[EntityId],
    transforms: &[Mat3],
) {
    let Some(document) = document else {
        return;
    };
    if ids.len() * transforms.len() > PREVIEW_LIMIT {
        return;
    }
    for transform in transforms {
        for &id in ids {
            let Some(entity) = document.model().entities.find(id) else {
                continue;
            };
            if let Ok(moved) = transformed(&entity.geometry, transform) {
                feedback.shapes.push(moved);
            }
        }
    }
}

// The rubber band line from a base point to the cursor, with the base marked.
fn add_band(feedback: &mut ToolFeedback, from: Point2, cursor: Point2) {
    feedback.markers.push(from);
    if !coincident(from, cursor) {
        feedback
            .shapes
            .push(Geometry::Line(Segment2::new(from, cursor)));
    }
}

// A tool's undo history: the state before each accepted input. Undo inside a
// tool puts the previous one back, so every step - a picked point, an option
// switched on - is undone the same way and none can be forgotten.
#[derive(Default)]
struct History<State> {
    now: State,
    past: Vec<State>,
}

impl<State: Clone> History<State> {
    fn remember(&mut self) {
        self.past.push(self.now.clone());
    }

    fn back(&mut self) -> bool {
        match self.past.pop() {
            Some(before) => {
                self.now = before;
                true
            }
            None => false,
        }
    }
}

// ---- the selection every tool here acts on ------------------------------------------
//
// Taken from the start when there is one. Otherwise the tool asks for it
// first (ToolInput::Selection): the view selects as the Select tool does, and
// may also hand single picks to entity(); Enter takes both - the document's
// selection at that moment and the picks - so the tool works whichever way the
// view reports them. "All" selects everything selectable.

/// What a tool's steps are lent for one call: the document now, the
/// selection the tool acts on, and the last cursor the view previewed (for
/// direct distance entry: preview() is the only place a tool sees the
/// cursor).
struct Chosen<'a> {
    document: Option<&'a Document>,
    ids: &'a [EntityId],
    cursor: Option<Point2>,
}

impl Chosen<'_> {
    fn add_preview(&self, feedback: &mut ToolFeedback, transform: &Mat3) {
        add_transformed(
            feedback,
            self.document,
            self.ids,
            std::slice::from_ref(transform),
        );
    }
}

/// A transform tool's own dialogue, after the selection (the C++
/// SelectionTool's virtual step methods). The defaults answer as
/// InteractiveTool's do.
trait TransformSteps {
    fn step_prompt(&self) -> String;

    fn step_expects(&self) -> ToolInput {
        ToolInput::Point
    }

    fn step_point(&mut self, cx: &ToolContext<'_>, chosen: &Chosen<'_>, at: Point2) -> ToolStep {
        let _ = chosen;
        DefaultAnswers(self.step_expects()).point(cx, at)
    }

    fn step_value(&mut self, cx: &ToolContext<'_>, chosen: &Chosen<'_>, text: &str) -> ToolStep {
        let _ = chosen;
        DefaultAnswers(self.step_expects()).value(cx, text)
    }

    fn step_enter(&mut self, cx: &ToolContext<'_>, chosen: &Chosen<'_>) -> ToolStep {
        let _ = chosen;
        DefaultAnswers(self.step_expects()).enter(cx)
    }

    /// Steps back one input; false when the tool is at its first step.
    fn step_undo(&mut self) -> bool {
        false
    }

    fn step_preview(&self, chosen: &Chosen<'_>, cursor: Point2) -> ToolFeedback {
        let _ = (chosen, cursor);
        ToolFeedback::default()
    }

    fn step_last_point(&self) -> Option<Point2> {
        None
    }

    /// Esc after the selection: the default keeps nothing, as every tool here
    /// but Copy makes its one command in a single step.
    fn step_cancel(&mut self, chosen: &Chosen<'_>) -> ToolStep {
        let _ = chosen;
        ToolStep::done(None, "", false)
    }

    /// The selection is complete. Erase finishes here; the others go on.
    fn selected(&mut self, chosen: &Chosen<'_>) -> ToolStep {
        ToolStep::next(format!("{} selected", entities(chosen.ids.len())))
    }
}

struct SelectionTool<Steps> {
    /// Completes "Select entities to ...".
    verb: &'static str,
    selecting: bool,
    asked: bool,
    ids: Vec<EntityId>,
    picked: Vec<EntityId>,
    pick_history: Vec<Vec<EntityId>>,
    /// The last cursor the view previewed (the C++'s mutable cursor_).
    cursor: Cell<Option<Point2>>,
    steps: Steps,
}

impl<Steps: TransformSteps> SelectionTool<Steps> {
    // A tool that `confirms` starts in the selection step even when there is a
    // selection, so one Enter carries it out: Erase, which has nothing else to
    // ask. It does NOT copy that selection into its picks. The selection lives
    // in the document, where Enter reads it, and the view changes it there as
    // the Select tool does - a plain click replaces it. A copy held here could
    // be neither seen nor taken back, and Erase would delete what the user had
    // just deselected.
    fn new(start: &ToolStart, verb: &'static str, confirms: bool, steps: Steps) -> Self {
        let ask = start.selection.is_empty() || confirms;
        Self {
            verb,
            selecting: ask,
            asked: ask,
            ids: if ask {
                Vec::new()
            } else {
                start.selection.clone()
            },
            picked: Vec::new(),
            pick_history: Vec::new(),
            cursor: Cell::new(None),
            steps,
        }
    }
}

impl<Steps: TransformSteps> InteractiveTool for SelectionTool<Steps> {
    fn prompt(&self, _cx: &ToolContext<'_>) -> String {
        if self.selecting {
            format!(
                "Select entities to {} or [All], then press Enter",
                self.verb
            )
        } else {
            self.steps.step_prompt()
        }
    }

    fn expects(&self, _cx: &ToolContext<'_>) -> ToolInput {
        if self.selecting {
            ToolInput::Selection
        } else {
            self.steps.step_expects()
        }
    }

    fn point(&mut self, cx: &ToolContext<'_>, at: Point2) -> ToolStep {
        if self.selecting {
            return ToolStep::rejected(format!(
                "select the entities to {} first, then press Enter",
                self.verb
            ));
        }
        let chosen = Chosen {
            document: cx.document,
            ids: &self.ids,
            cursor: self.cursor.get(),
        };
        self.steps.step_point(cx, &chosen, at)
    }

    fn entity(&mut self, cx: &ToolContext<'_>, id: EntityId, at: Point2) -> ToolStep {
        if !self.selecting {
            return DefaultAnswers(self.expects(cx)).entity(cx, id, at);
        }
        if !cx
            .document
            .is_some_and(|document| document.model().entities.contains(id))
        {
            return ToolStep::rejected("that entity is not in the drawing");
        }
        if self.picked.contains(&id) {
            return ToolStep::rejected("that entity is already selected");
        }
        self.pick_history.push(self.picked.clone());
        self.picked.push(id);
        ToolStep::next("")
    }

    fn value(&mut self, cx: &ToolContext<'_>, text: &str) -> ToolStep {
        if !self.selecting {
            let chosen = Chosen {
                document: cx.document,
                ids: &self.ids,
                cursor: self.cursor.get(),
            };
            return self.steps.step_value(cx, &chosen, text);
        }
        if !is_option(text, "A", "ALL") {
            return not_understood(text, "an option here: pick entities, or type All");
        }
        let Some(document) = cx.document else {
            return ToolStep::rejected("there is no drawing to select from");
        };
        let mut all = Vec::new();
        let model = document.model();
        // The document rule alone, as the C++ has it: its comment says the
        // ToolContext carries no view, so a layer hidden in the view the tool
        // runs in is not excluded yet, although selection.rs says it must be.
        // The context has the view now (ToolContext::view); the C++ does not
        // use it here, nor for Stretch's window, and the port keeps that.
        model.entities.for_each(|candidate| {
            if is_selectable(model, candidate, &NO_LAYER_OVERRIDES) {
                all.push(candidate.id);
            }
        });
        self.pick_history
            .push(std::mem::replace(&mut self.picked, all));
        ToolStep::next(format!("{} selected", entities(self.picked.len())))
    }

    fn enter(&mut self, cx: &ToolContext<'_>) -> ToolStep {
        if !self.selecting {
            let chosen = Chosen {
                document: cx.document,
                ids: &self.ids,
                cursor: self.cursor.get(),
            };
            return self.steps.step_enter(cx, &chosen);
        }
        let mut chosen = self.picked.clone();
        if let Some(document) = cx.document {
            chosen.extend(document.selection().ids());
            chosen.retain(|&id| document.model().entities.contains(id));
        }
        chosen.sort_unstable();
        chosen.dedup();
        if chosen.is_empty() {
            return ToolStep::rejected(format!(
                "nothing is selected: select the entities to {}, then press Enter",
                self.verb
            ));
        }
        self.ids = chosen;
        self.selecting = false;
        let chosen = Chosen {
            document: cx.document,
            ids: &self.ids,
            cursor: self.cursor.get(),
        };
        self.steps.selected(&chosen)
    }

    fn undo(&mut self, _cx: &ToolContext<'_>) -> ToolStep {
        if self.selecting {
            let Some(before) = self.pick_history.pop() else {
                return ToolStep::rejected("nothing to undo in this tool");
            };
            self.picked = before;
            return ToolStep::next("");
        }
        if self.steps.step_undo() {
            return ToolStep::next("");
        }
        if self.asked {
            // Back to the selection the tool asked for, with the picks kept.
            self.selecting = true;
            self.ids.clear();
            return ToolStep::next("");
        }
        ToolStep::rejected("nothing to undo in this tool")
    }

    fn preview(&self, cx: &ToolContext<'_>, cursor: Point2) -> ToolFeedback {
        self.cursor.set(Some(cursor));
        if self.selecting {
            return ToolFeedback::default();
        }
        let chosen = Chosen {
            document: cx.document,
            ids: &self.ids,
            cursor: self.cursor.get(),
        };
        self.steps.step_preview(&chosen, cursor)
    }

    fn last_point(&self, _cx: &ToolContext<'_>) -> Option<Point2> {
        if self.selecting {
            None
        } else {
            self.steps.step_last_point()
        }
    }

    // Nothing is made while the selection is being given; after it, the
    // tool's own step says what Esc keeps.
    fn cancel(&mut self, cx: &ToolContext<'_>) -> ToolStep {
        if self.selecting {
            return ToolStep::done(None, "", false);
        }
        let chosen = Chosen {
            document: cx.document,
            ids: &self.ids,
            cursor: self.cursor.get(),
        };
        self.steps.step_cancel(&chosen)
    }
}

// ---- Move ----------------------------------------------------------------------------

// Where Move, Copy and Stretch are told to go: a base point then a second point,
// or a displacement typed as dx,dy. Enter at the base point asks for the
// displacement, and Enter at the second point uses the base point AS the
// displacement - AutoCAD's two defaults.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Displace {
    #[default]
    Base,
    Typed,
    Second,
}

fn displacement_prompt(step: Displace) -> String {
    String::from(match step {
        Displace::Base => "Specify base point or [Displacement] <Displacement>",
        Displace::Typed => "Specify displacement as dx,dy",
        Displace::Second => "Specify second point or <use first point as displacement>",
    })
}

#[derive(Clone, Copy, Debug, Default)]
struct MoveState {
    step: Displace,
    base: Point2,
}

#[derive(Default)]
struct MoveSteps {
    state: History<MoveState>,
}

impl MoveSteps {
    fn finish(chosen: &Chosen<'_>, delta: Vec2) -> ToolStep {
        if delta.length() <= tolerance::GEOMETRIC {
            return ToolStep::rejected("a displacement of zero would move nothing");
        }
        ToolStep::done(
            Some(move_entities(chosen.ids.to_vec(), delta)),
            format!("{} moved", entities(chosen.ids.len())),
            false,
        )
    }
}

impl TransformSteps for MoveSteps {
    fn step_prompt(&self) -> String {
        displacement_prompt(self.state.now.step)
    }

    fn step_point(&mut self, _cx: &ToolContext<'_>, chosen: &Chosen<'_>, at: Point2) -> ToolStep {
        match self.state.now.step {
            Displace::Base => {
                self.state.remember();
                self.state.now.base = at;
                self.state.now.step = Displace::Second;
                ToolStep::next("")
            }
            Displace::Typed => Self::finish(chosen, at - ORIGIN),
            Displace::Second => Self::finish(chosen, at - self.state.now.base),
        }
    }

    fn step_value(&mut self, _cx: &ToolContext<'_>, chosen: &Chosen<'_>, text: &str) -> ToolStep {
        if self.state.now.step == Displace::Base && is_option(text, "D", "DISPLACEMENT") {
            self.state.remember();
            self.state.now.step = Displace::Typed;
            return ToolStep::next("");
        }
        if self.state.now.step == Displace::Second
            && let Some(distance) = number_in(text)
        {
            let base = self.state.now.base;
            let Some(to) = along_cursor(distance, base, chosen.cursor) else {
                return ToolStep::rejected(NO_DIRECTION);
            };
            return Self::finish(chosen, to - base);
        }
        not_understood(text, "a point, a distance or an option here")
    }

    fn step_enter(&mut self, _cx: &ToolContext<'_>, chosen: &Chosen<'_>) -> ToolStep {
        match self.state.now.step {
            Displace::Base => {
                self.state.remember();
                self.state.now.step = Displace::Typed;
                ToolStep::next("")
            }
            Displace::Typed => ToolStep::rejected("type the displacement as dx,dy"),
            Displace::Second => Self::finish(chosen, self.state.now.base - ORIGIN),
        }
    }

    fn step_undo(&mut self) -> bool {
        self.state.back()
    }

    fn step_preview(&self, chosen: &Chosen<'_>, cursor: Point2) -> ToolFeedback {
        let mut feedback = ToolFeedback::default();
        let now = self.state.now;
        if now.step == Displace::Second {
            chosen.add_preview(&mut feedback, &Mat3::translation(cursor - now.base));
            add_band(&mut feedback, now.base, cursor);
        }
        feedback
    }

    fn step_last_point(&self) -> Option<Point2> {
        match self.state.now.step {
            // "@3,4" typed as a displacement means 3,4 from nowhere in
            // particular.
            Displace::Typed => Some(ORIGIN),
            Displace::Second => Some(self.state.now.base),
            Displace::Base => None,
        }
    }
}

// ---- Copy ----------------------------------------------------------------------------

// One base point, then as many second points as the user wants, each a copy;
// Enter ends. ALL the copies are one command, so one undo removes them all -
// the old Copy tool made one command per copy.
#[derive(Clone, Debug, Default)]
struct CopyState {
    step: Displace,
    base: Point2,
    /// One per copy placed so far.
    offsets: Vec<Vec2>,
}

#[derive(Default)]
struct CopySteps {
    state: History<CopyState>,
}

impl CopySteps {
    fn place(&mut self, at: Point2) -> ToolStep {
        if coincident(at, self.state.now.base) {
            return ToolStep::rejected(
                "the second point is the base point: the copy would lie on the original",
            );
        }
        self.state.remember();
        let offset = at - self.state.now.base;
        self.state.now.offsets.push(offset);
        ToolStep::next("")
    }

    fn exit(&self, chosen: &Chosen<'_>) -> ToolStep {
        if self.state.now.offsets.is_empty() {
            return ToolStep::done(None, "nothing copied", false);
        }
        Self::finish(chosen, &self.state.now.offsets)
    }

    fn finish(chosen: &Chosen<'_>, offsets: &[Vec2]) -> ToolStep {
        let transforms = offsets
            .iter()
            .map(|&offset| Mat3::translation(offset))
            .collect();
        let copies = format!(
            "{}{}",
            offsets.len(),
            if offsets.len() == 1 {
                " copy"
            } else {
                " copies"
            }
        );
        ToolStep::done(
            Some(duplicate_entities("COPY", chosen.ids.to_vec(), transforms)),
            format!("{copies} of {}", entities(chosen.ids.len())),
            false,
        )
    }
}

impl TransformSteps for CopySteps {
    fn step_prompt(&self) -> String {
        if self.state.now.step == Displace::Second && !self.state.now.offsets.is_empty() {
            return String::from("Specify second point or [Exit/Undo] <Exit>");
        }
        displacement_prompt(self.state.now.step)
    }

    fn step_point(&mut self, _cx: &ToolContext<'_>, chosen: &Chosen<'_>, at: Point2) -> ToolStep {
        match self.state.now.step {
            Displace::Base => {
                self.state.remember();
                self.state.now.base = at;
                self.state.now.step = Displace::Second;
                ToolStep::next("")
            }
            Displace::Typed => {
                let offset = at - ORIGIN;
                if offset.length() <= tolerance::GEOMETRIC {
                    return ToolStep::rejected(
                        "a displacement of zero would put the copy on the original",
                    );
                }
                Self::finish(chosen, &[offset])
            }
            Displace::Second => self.place(at),
        }
    }

    fn step_value(&mut self, _cx: &ToolContext<'_>, chosen: &Chosen<'_>, text: &str) -> ToolStep {
        let step = self.state.now.step;
        if step == Displace::Base && is_option(text, "D", "DISPLACEMENT") {
            self.state.remember();
            self.state.now.step = Displace::Typed;
            return ToolStep::next("");
        }
        if step == Displace::Second {
            if is_option(text, "E", "EXIT") {
                return self.exit(chosen);
            }
            if is_option(text, "U", "UNDO") {
                return if self.state.back() {
                    ToolStep::next("")
                } else {
                    ToolStep::rejected("nothing to undo in this tool")
                };
            }
            if let Some(distance) = number_in(text) {
                let Some(to) = along_cursor(distance, self.state.now.base, chosen.cursor) else {
                    return ToolStep::rejected(NO_DIRECTION);
                };
                return self.place(to);
            }
        }
        not_understood(text, "a point, a distance or an option here")
    }

    fn step_enter(&mut self, _cx: &ToolContext<'_>, chosen: &Chosen<'_>) -> ToolStep {
        match self.state.now.step {
            Displace::Base => {
                self.state.remember();
                self.state.now.step = Displace::Typed;
                ToolStep::next("")
            }
            Displace::Typed => ToolStep::rejected("type the displacement as dx,dy"),
            Displace::Second => {
                if self.state.now.offsets.is_empty() {
                    let offset = self.state.now.base - ORIGIN;
                    if offset.length() <= tolerance::GEOMETRIC {
                        return ToolStep::rejected(
                            "the base point is the origin, so as a displacement it would put the \
                             copy on the original",
                        );
                    }
                    return Self::finish(chosen, &[offset]);
                }
                self.exit(chosen)
            }
        }
    }

    fn step_undo(&mut self) -> bool {
        self.state.back()
    }

    // Esc keeps the copies already placed: each was a click the user meant,
    // and dropping them all for want of an Enter lost work (the tool host's
    // old rule had to leave Copy out, because its Enter with none placed yet
    // copies by the base point - a default Esc must not apply).
    fn step_cancel(&mut self, chosen: &Chosen<'_>) -> ToolStep {
        if self.state.now.step == Displace::Second && !self.state.now.offsets.is_empty() {
            return Self::finish(chosen, &self.state.now.offsets);
        }
        ToolStep::done(None, "", false)
    }

    fn step_preview(&self, chosen: &Chosen<'_>, cursor: Point2) -> ToolFeedback {
        let mut feedback = ToolFeedback::default();
        let now = &self.state.now;
        if now.step != Displace::Second {
            return feedback;
        }
        // The copies placed so far are not in the drawing until Enter, so the
        // rubber band shows them too, or the user would lose sight of them.
        let mut transforms: Vec<Mat3> = now
            .offsets
            .iter()
            .map(|&offset| Mat3::translation(offset))
            .collect();
        transforms.push(Mat3::translation(cursor - now.base));
        add_transformed(&mut feedback, chosen.document, chosen.ids, &transforms);
        add_band(&mut feedback, now.base, cursor);
        feedback
    }

    fn step_last_point(&self) -> Option<Point2> {
        match self.state.now.step {
            Displace::Typed => Some(ORIGIN),
            Displace::Second => Some(self.state.now.base),
            Displace::Base => None,
        }
    }
}

// ---- Rotate --------------------------------------------------------------------------

// A base point, then the angle - typed in degrees counter-clockwise, or picked
// as the direction from the base point. Reference: the angle the selection is
// at now (typed, or two points along it), then the angle it should be at
// (typed, picked from the base point, or two Points); it turns by the
// difference - "make this edge run due north". Copy rotates a copy instead.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum RotateStep {
    #[default]
    Base,
    Angle,
    ReferenceFirst,
    ReferenceSecond,
    NewAngle,
    NewFirst,
    NewSecond,
}

#[derive(Clone, Copy, Debug, Default)]
struct RotateState {
    step: RotateStep,
    base: Point2,
    copy: bool,
    /// Radians.
    reference: f64,
    /// Of a two-point reference or new angle.
    first: Point2,
}

#[derive(Default)]
struct RotateSteps {
    state: History<RotateState>,
}

impl RotateSteps {
    fn advance(&mut self, change: impl FnOnce(&mut RotateState)) -> ToolStep {
        self.state.remember();
        change(&mut self.state.now);
        ToolStep::next("")
    }

    fn finish(&self, chosen: &Chosen<'_>, radians: f64) -> ToolStep {
        if angles_equal(radians, 0.0, tolerance::ANGULAR) {
            return ToolStep::rejected(
                "a rotation of 0 degrees, or of whole turns, changes nothing",
            );
        }
        let now = self.state.now;
        let ids = chosen.ids.to_vec();
        let command = if now.copy {
            duplicate_entities("ROTATE", ids, vec![Mat3::rotation_about(now.base, radians)])
        } else {
            rotate_entities(ids, now.base, radians)
        };
        let what = if now.copy {
            " copied and rotated"
        } else {
            " rotated"
        };
        ToolStep::done(
            Some(command),
            format!("{}{what}", entities(chosen.ids.len())),
            false,
        )
    }
}

impl TransformSteps for RotateSteps {
    fn step_prompt(&self) -> String {
        String::from(match self.state.now.step {
            RotateStep::Base => "Specify base point",
            RotateStep::Angle => "Specify rotation angle or [Copy/Reference]",
            RotateStep::ReferenceFirst => "Specify the reference angle or its first point <0>",
            RotateStep::ReferenceSecond => "Specify second point of the reference angle",
            RotateStep::NewAngle => "Specify the new angle or [Points]",
            RotateStep::NewFirst => "Specify first point of the new angle",
            RotateStep::NewSecond => "Specify second point of the new angle",
        })
    }

    fn step_point(&mut self, _cx: &ToolContext<'_>, chosen: &Chosen<'_>, at: Point2) -> ToolStep {
        let now = self.state.now;
        match now.step {
            RotateStep::Base => self.advance(|next| {
                next.base = at;
                next.step = RotateStep::Angle;
            }),
            RotateStep::Angle => {
                if coincident(at, now.base) {
                    return ToolStep::rejected(
                        "the point is on the base point, so it gives no angle",
                    );
                }
                self.finish(chosen, (at - now.base).angle())
            }
            RotateStep::ReferenceFirst => self.advance(|next| {
                next.first = at;
                next.step = RotateStep::ReferenceSecond;
            }),
            RotateStep::ReferenceSecond => {
                if coincident(at, now.first) {
                    return ToolStep::rejected("the two points coincide, so they give no angle");
                }
                self.advance(|next| {
                    next.reference = (at - next.first).angle();
                    next.step = RotateStep::NewAngle;
                })
            }
            RotateStep::NewAngle => {
                if coincident(at, now.base) {
                    return ToolStep::rejected(
                        "the point is on the base point, so it gives no angle",
                    );
                }
                self.finish(chosen, (at - now.base).angle() - now.reference)
            }
            RotateStep::NewFirst => self.advance(|next| {
                next.first = at;
                next.step = RotateStep::NewSecond;
            }),
            RotateStep::NewSecond => {
                if coincident(at, now.first) {
                    return ToolStep::rejected("the two points coincide, so they give no angle");
                }
                self.finish(chosen, (at - now.first).angle() - now.reference)
            }
        }
    }

    fn step_value(&mut self, _cx: &ToolContext<'_>, chosen: &Chosen<'_>, text: &str) -> ToolStep {
        let now = self.state.now;
        let degrees = number_in(text);
        match now.step {
            RotateStep::Angle => {
                if let Some(degrees) = degrees {
                    return self.finish(chosen, degrees * DEG_TO_RAD);
                }
                if is_option(text, "C", "COPY") {
                    self.state.remember();
                    self.state.now.copy = !self.state.now.copy;
                    return ToolStep::next(if self.state.now.copy {
                        "Rotating a copy of the selection."
                    } else {
                        "Rotating the selection itself."
                    });
                }
                if is_option(text, "R", "REFERENCE") {
                    return self.advance(|next| next.step = RotateStep::ReferenceFirst);
                }
            }
            RotateStep::ReferenceFirst => {
                if let Some(degrees) = degrees {
                    return self.advance(|next| {
                        next.reference = degrees * DEG_TO_RAD;
                        next.step = RotateStep::NewAngle;
                    });
                }
            }
            RotateStep::NewAngle => {
                if let Some(degrees) = degrees {
                    return self.finish(chosen, degrees * DEG_TO_RAD - now.reference);
                }
                if is_option(text, "P", "POINTS") {
                    return self.advance(|next| next.step = RotateStep::NewFirst);
                }
            }
            RotateStep::Base
            | RotateStep::ReferenceSecond
            | RotateStep::NewFirst
            | RotateStep::NewSecond => {}
        }
        not_understood(text, "an angle, a point or an option here")
    }

    fn step_enter(&mut self, _cx: &ToolContext<'_>, _chosen: &Chosen<'_>) -> ToolStep {
        if self.state.now.step == RotateStep::ReferenceFirst {
            // <0>: the reference is due east.
            return self.advance(|next| {
                next.reference = 0.0;
                next.step = RotateStep::NewAngle;
            });
        }
        ToolStep::rejected(format!("this step has no default: {}", self.step_prompt()))
    }

    fn step_undo(&mut self) -> bool {
        self.state.back()
    }

    fn step_preview(&self, chosen: &Chosen<'_>, cursor: Point2) -> ToolFeedback {
        let mut feedback = ToolFeedback::default();
        let now = self.state.now;
        let turn = |feedback: &mut ToolFeedback, from: Point2, minus: f64| {
            if !coincident(cursor, from) {
                chosen.add_preview(
                    feedback,
                    &Mat3::rotation_about(now.base, (cursor - from).angle() - minus),
                );
            }
            add_band(feedback, from, cursor);
        };
        match now.step {
            RotateStep::Angle => turn(&mut feedback, now.base, 0.0),
            RotateStep::NewAngle => turn(&mut feedback, now.base, now.reference),
            RotateStep::NewSecond => {
                turn(&mut feedback, now.first, now.reference);
                feedback.markers.push(now.base);
            }
            RotateStep::ReferenceSecond => {
                add_band(&mut feedback, now.first, cursor);
                feedback.markers.push(now.base);
            }
            RotateStep::ReferenceFirst | RotateStep::NewFirst => feedback.markers.push(now.base),
            RotateStep::Base => {}
        }
        feedback
    }

    fn step_last_point(&self) -> Option<Point2> {
        let now = self.state.now;
        match now.step {
            RotateStep::Base => None,
            RotateStep::ReferenceSecond | RotateStep::NewSecond => Some(now.first),
            RotateStep::Angle
            | RotateStep::ReferenceFirst
            | RotateStep::NewAngle
            | RotateStep::NewFirst => Some(now.base),
        }
    }
}

// ---- Scale ---------------------------------------------------------------------------

// A base point, then the factor - typed, or picked as the distance from the
// base point. Reference: the length something is now (typed, or two points),
// then the length it should be (typed, picked from the base point, or two
// Points); the factor is their ratio - "make this 7.3 m edge 8 m". Copy scales
// a copy instead.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum ScaleStep {
    #[default]
    Base,
    Factor,
    ReferenceFirst,
    ReferenceSecond,
    NewLength,
    NewFirst,
    NewSecond,
}

#[derive(Clone, Copy, Debug)]
struct ScaleState {
    step: ScaleStep,
    base: Point2,
    copy: bool,
    reference: f64,
    first: Point2,
}

impl Default for ScaleState {
    fn default() -> Self {
        Self {
            step: ScaleStep::Base,
            base: Point2::default(),
            copy: false,
            reference: 1.0,
            first: Point2::default(),
        }
    }
}

#[derive(Default)]
struct ScaleSteps {
    state: History<ScaleState>,
}

impl ScaleSteps {
    fn advance(&mut self, change: impl FnOnce(&mut ScaleState)) -> ToolStep {
        self.state.remember();
        change(&mut self.state.now);
        ToolStep::next("")
    }

    fn finish(&self, chosen: &Chosen<'_>, factor: f64) -> ToolStep {
        // Zero collapses the selection onto the base point and a negative
        // factor is a half-turn in disguise; the scale command refuses both,
        // and so does the tool, before it gets there.
        if !(factor.is_finite() && factor > tolerance::GEOMETRIC) {
            return ToolStep::rejected("the scale factor must be greater than zero");
        }
        if nearly_equal(factor, 1.0, tolerance::RELATIVE) {
            return ToolStep::rejected("a scale factor of 1 changes nothing");
        }
        let now = self.state.now;
        let ids = chosen.ids.to_vec();
        let command = if now.copy {
            duplicate_entities(
                "SCALE",
                ids,
                vec![Mat3::scaling_about(now.base, factor, factor)],
            )
        } else {
            scale_entities(ids, now.base, factor)
        };
        let what = if now.copy {
            " copied and scaled"
        } else {
            " scaled"
        };
        ToolStep::done(
            Some(command),
            format!("{}{what}", entities(chosen.ids.len())),
            false,
        )
    }
}

impl TransformSteps for ScaleSteps {
    fn step_prompt(&self) -> String {
        String::from(match self.state.now.step {
            ScaleStep::Base => "Specify base point",
            ScaleStep::Factor => "Specify scale factor or [Copy/Reference]",
            ScaleStep::ReferenceFirst => "Specify reference length or its first point <1>",
            ScaleStep::ReferenceSecond => "Specify second point of the reference length",
            ScaleStep::NewLength => "Specify new length or [Points]",
            ScaleStep::NewFirst => "Specify first point of the new length",
            ScaleStep::NewSecond => "Specify second point of the new length",
        })
    }

    fn step_point(&mut self, _cx: &ToolContext<'_>, chosen: &Chosen<'_>, at: Point2) -> ToolStep {
        let now = self.state.now;
        match now.step {
            ScaleStep::Base => self.advance(|next| {
                next.base = at;
                next.step = ScaleStep::Factor;
            }),
            ScaleStep::Factor => self.finish(chosen, now.base.distance_to(at)),
            ScaleStep::ReferenceFirst => self.advance(|next| {
                next.first = at;
                next.step = ScaleStep::ReferenceSecond;
            }),
            ScaleStep::ReferenceSecond => {
                let length = now.first.distance_to(at);
                if length <= tolerance::GEOMETRIC {
                    return ToolStep::rejected(
                        "the two points coincide: a reference length of zero cannot be scaled \
                         from",
                    );
                }
                self.advance(|next| {
                    next.reference = length;
                    next.step = ScaleStep::NewLength;
                })
            }
            ScaleStep::NewLength => self.finish(chosen, now.base.distance_to(at) / now.reference),
            ScaleStep::NewFirst => self.advance(|next| {
                next.first = at;
                next.step = ScaleStep::NewSecond;
            }),
            ScaleStep::NewSecond => self.finish(chosen, now.first.distance_to(at) / now.reference),
        }
    }

    fn step_value(&mut self, _cx: &ToolContext<'_>, chosen: &Chosen<'_>, text: &str) -> ToolStep {
        let now = self.state.now;
        let number = number_in(text);
        match now.step {
            ScaleStep::Factor => {
                if let Some(number) = number {
                    return self.finish(chosen, number);
                }
                if is_option(text, "C", "COPY") {
                    self.state.remember();
                    self.state.now.copy = !self.state.now.copy;
                    return ToolStep::next(if self.state.now.copy {
                        "Scaling a copy of the selection."
                    } else {
                        "Scaling the selection itself."
                    });
                }
                if is_option(text, "R", "REFERENCE") {
                    return self.advance(|next| next.step = ScaleStep::ReferenceFirst);
                }
            }
            ScaleStep::ReferenceFirst => {
                if let Some(number) = number {
                    if !(number > tolerance::GEOMETRIC) {
                        return ToolStep::rejected(
                            "the reference length must be greater than zero",
                        );
                    }
                    return self.advance(|next| {
                        next.reference = number;
                        next.step = ScaleStep::NewLength;
                    });
                }
            }
            ScaleStep::NewLength => {
                if let Some(number) = number {
                    return self.finish(chosen, number / now.reference);
                }
                if is_option(text, "P", "POINTS") {
                    return self.advance(|next| next.step = ScaleStep::NewFirst);
                }
            }
            ScaleStep::Base
            | ScaleStep::ReferenceSecond
            | ScaleStep::NewFirst
            | ScaleStep::NewSecond => {}
        }
        not_understood(text, "a factor, a length, a point or an option here")
    }

    fn step_enter(&mut self, _cx: &ToolContext<'_>, _chosen: &Chosen<'_>) -> ToolStep {
        if self.state.now.step == ScaleStep::ReferenceFirst {
            return self.advance(|next| {
                next.reference = 1.0;
                next.step = ScaleStep::NewLength;
            });
        }
        ToolStep::rejected(format!("this step has no default: {}", self.step_prompt()))
    }

    fn step_undo(&mut self) -> bool {
        self.state.back()
    }

    fn step_preview(&self, chosen: &Chosen<'_>, cursor: Point2) -> ToolFeedback {
        let mut feedback = ToolFeedback::default();
        let now = self.state.now;
        let grow = |feedback: &mut ToolFeedback, from: Point2| {
            let per = if now.step == ScaleStep::Factor {
                1.0
            } else {
                now.reference
            };
            let factor = from.distance_to(cursor) / per;
            if factor > tolerance::GEOMETRIC {
                chosen.add_preview(feedback, &Mat3::scaling_about(now.base, factor, factor));
            }
            add_band(feedback, from, cursor);
        };
        match now.step {
            ScaleStep::Factor | ScaleStep::NewLength => grow(&mut feedback, now.base),
            ScaleStep::NewSecond => {
                grow(&mut feedback, now.first);
                feedback.markers.push(now.base);
            }
            ScaleStep::ReferenceSecond => {
                add_band(&mut feedback, now.first, cursor);
                feedback.markers.push(now.base);
            }
            ScaleStep::ReferenceFirst | ScaleStep::NewFirst => feedback.markers.push(now.base),
            ScaleStep::Base => {}
        }
        feedback
    }

    fn step_last_point(&self) -> Option<Point2> {
        let now = self.state.now;
        match now.step {
            ScaleStep::Base => None,
            ScaleStep::ReferenceSecond | ScaleStep::NewSecond => Some(now.first),
            ScaleStep::Factor
            | ScaleStep::ReferenceFirst
            | ScaleStep::NewLength
            | ScaleStep::NewFirst => Some(now.base),
        }
    }
}

// ---- Mirror --------------------------------------------------------------------------

// Two points on the mirror line, then whether to erase the source - No by
// default, as in AutoCAD, so the usual result is the symmetric pair. (The
// command line's MIRROR replaces unless told KEEP; its default is the
// opposite dialogue, not a different operation.)
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum MirrorStep {
    #[default]
    First,
    Second,
    EraseSource,
}

#[derive(Clone, Copy, Debug, Default)]
struct MirrorState {
    step: MirrorStep,
    first: Point2,
    second: Point2,
}

#[derive(Default)]
struct MirrorSteps {
    state: History<MirrorState>,
}

impl MirrorSteps {
    fn finish(&self, chosen: &Chosen<'_>, keep_source: bool) -> ToolStep {
        let now = self.state.now;
        ToolStep::done(
            Some(mirror_entities(
                chosen.ids.to_vec(),
                now.first,
                now.second,
                keep_source,
            )),
            format!(
                "{}{}",
                entities(chosen.ids.len()),
                if keep_source {
                    " mirrored, source kept"
                } else {
                    " mirrored"
                }
            ),
            false,
        )
    }
}

impl TransformSteps for MirrorSteps {
    fn step_prompt(&self) -> String {
        String::from(match self.state.now.step {
            MirrorStep::First => "Specify first point of mirror line",
            MirrorStep::Second => "Specify second point of mirror line",
            MirrorStep::EraseSource => "Erase source entities? [Yes/No] <No>",
        })
    }

    fn step_expects(&self) -> ToolInput {
        if self.state.now.step == MirrorStep::EraseSource {
            ToolInput::Value
        } else {
            ToolInput::Point
        }
    }

    fn step_point(&mut self, _cx: &ToolContext<'_>, _chosen: &Chosen<'_>, at: Point2) -> ToolStep {
        match self.state.now.step {
            MirrorStep::First => {
                self.state.remember();
                self.state.now.first = at;
                self.state.now.step = MirrorStep::Second;
                ToolStep::next("")
            }
            MirrorStep::Second => {
                if coincident(at, self.state.now.first) {
                    return ToolStep::rejected("the mirror line needs two different points");
                }
                self.state.remember();
                self.state.now.second = at;
                self.state.now.step = MirrorStep::EraseSource;
                ToolStep::next("")
            }
            MirrorStep::EraseSource => ToolStep::rejected("answer Yes or No"),
        }
    }

    fn step_value(&mut self, _cx: &ToolContext<'_>, chosen: &Chosen<'_>, text: &str) -> ToolStep {
        if self.state.now.step == MirrorStep::EraseSource {
            if is_option(text, "Y", "YES") {
                return self.finish(chosen, false);
            }
            if is_option(text, "N", "NO") {
                return self.finish(chosen, true);
            }
            return not_understood(text, "an answer: type Yes or No");
        }
        not_understood(text, "a point")
    }

    fn step_enter(&mut self, _cx: &ToolContext<'_>, chosen: &Chosen<'_>) -> ToolStep {
        if self.state.now.step == MirrorStep::EraseSource {
            return self.finish(chosen, true);
        }
        ToolStep::rejected(format!("this step has no default: {}", self.step_prompt()))
    }

    fn step_undo(&mut self) -> bool {
        self.state.back()
    }

    fn step_preview(&self, chosen: &Chosen<'_>, cursor: Point2) -> ToolFeedback {
        let mut feedback = ToolFeedback::default();
        let now = self.state.now;
        let reflect = |feedback: &mut ToolFeedback, second: Point2| {
            if !coincident(now.first, second) {
                chosen.add_preview(feedback, &Mat3::reflection(now.first, second - now.first));
            }
            add_band(feedback, now.first, second);
        };
        if now.step == MirrorStep::Second {
            reflect(&mut feedback, cursor);
        } else if now.step == MirrorStep::EraseSource {
            reflect(&mut feedback, now.second);
        }
        feedback
    }

    fn step_last_point(&self) -> Option<Point2> {
        let now = self.state.now;
        match now.step {
            MirrorStep::First => None,
            MirrorStep::Second => Some(now.first),
            MirrorStep::EraseSource => Some(now.second),
        }
    }
}

// ---- Array: rectangular --------------------------------------------------------------

// Rows, columns, then the spacing - typed, or picked as a unit cell whose
// corners give the column and row spacing at once. A negative spacing builds
// the array to the left or downwards. The originals are cell (0, 0), as the
// command line's ARRAY has them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum ArrayStep {
    #[default]
    Rows,
    Columns,
    RowSpacing,
    CellCorner,
    ColumnSpacing,
    ColumnSecond,
}

#[derive(Clone, Copy, Debug)]
struct ArrayState {
    step: ArrayStep,
    rows: i32,
    columns: i32,
    row_spacing: f64,
    /// The unit cell's corner, or the first point of the column spacing.
    first: Point2,
}

impl Default for ArrayState {
    fn default() -> Self {
        Self {
            step: ArrayStep::Rows,
            rows: 1,
            columns: 1,
            row_spacing: 0.0,
            first: Point2::default(),
        }
    }
}

#[derive(Default)]
struct ArrayRectangularSteps {
    state: History<ArrayState>,
}

impl ArrayRectangularSteps {
    fn take_count(&mut self, count: i64) -> ToolStep {
        let now = self.state.now;
        if now.step == ArrayStep::Rows {
            if count > MAXIMUM_ARRAY_ITEMS {
                return ToolStep::rejected("an array is at most 100000 items");
            }
            self.state.remember();
            // At most MAXIMUM_ARRAY_ITEMS, so it fits.
            self.state.now.rows = count as i32;
            self.state.now.step = ArrayStep::Columns;
            return ToolStep::next("");
        }
        // Divided rather than multiplied: the count is whatever i64 was typed,
        // and count * rows can overflow - a wrapped, negative product would
        // pass as "under the limit". For whole numbers, count * rows > M
        // exactly when count > floor(M / rows).
        if count > MAXIMUM_ARRAY_ITEMS / i64::from(now.rows) {
            return ToolStep::rejected("an array is at most 100000 items");
        }
        if count == 1 && now.rows == 1 {
            return ToolStep::rejected(
                "one row of one column is the original alone: ask for more of either",
            );
        }
        self.state.remember();
        // At most MAXIMUM_ARRAY_ITEMS, so it fits.
        self.state.now.columns = count as i32;
        self.state.now.step = if now.rows > 1 {
            ArrayStep::RowSpacing
        } else {
            ArrayStep::ColumnSpacing
        };
        ToolStep::next("")
    }

    fn cells(&self, spacing: Vec2) -> Vec<Mat3> {
        let now = self.state.now;
        let mut out = Vec::new();
        for row in 0..now.rows {
            for column in 0..now.columns {
                if row != 0 || column != 0 {
                    out.push(Mat3::translation(Vec2::new(
                        spacing.x * f64::from(column),
                        spacing.y * f64::from(row),
                    )));
                }
            }
        }
        out
    }

    fn add_array(&self, feedback: &mut ToolFeedback, chosen: &Chosen<'_>, spacing: Vec2) {
        // Checked before the cells are built: a 1000 x 100 array would
        // otherwise make a hundred thousand matrices per mouse move only for
        // add_transformed to decline them. The product is at most
        // MAXIMUM_ARRAY_ITEMS (take_count), so it fits.
        let copies = (self.state.now.rows * self.state.now.columns - 1) as usize;
        if copies * chosen.ids.len() > PREVIEW_LIMIT {
            return;
        }
        let transforms = self.cells(spacing);
        add_transformed(feedback, chosen.document, chosen.ids, &transforms);
    }

    fn finish(&self, chosen: &Chosen<'_>, spacing: Vec2) -> ToolStep {
        let now = self.state.now;
        ToolStep::done(
            Some(array_entities(
                chosen.ids.to_vec(),
                now.rows,
                now.columns,
                spacing,
            )),
            format!(
                "{} x {} array of {}",
                now.rows,
                now.columns,
                entities(chosen.ids.len())
            ),
            false,
        )
    }
}

impl TransformSteps for ArrayRectangularSteps {
    fn step_prompt(&self) -> String {
        String::from(match self.state.now.step {
            ArrayStep::Rows => "Enter the number of rows <1>",
            ArrayStep::Columns => "Enter the number of columns <1>",
            ArrayStep::RowSpacing => "Enter the distance between rows or specify unit cell",
            ArrayStep::CellCorner => "Specify opposite corner of the unit cell",
            ArrayStep::ColumnSpacing => "Enter the distance between columns or its first point",
            ArrayStep::ColumnSecond => "Specify second point of the distance between columns",
        })
    }

    fn step_expects(&self) -> ToolInput {
        let step = self.state.now.step;
        if step == ArrayStep::Rows || step == ArrayStep::Columns {
            ToolInput::Value
        } else {
            ToolInput::Point
        }
    }

    fn step_value(&mut self, _cx: &ToolContext<'_>, chosen: &Chosen<'_>, text: &str) -> ToolStep {
        let now = self.state.now;
        match now.step {
            ArrayStep::Rows | ArrayStep::Columns => {
                let Some(count) = integer_in(text).filter(|&count| count >= 1) else {
                    return ToolStep::rejected(format!(
                        "the number of {} is a whole number, 1 or more",
                        if now.step == ArrayStep::Rows {
                            "rows"
                        } else {
                            "columns"
                        }
                    ));
                };
                self.take_count(count)
            }
            ArrayStep::RowSpacing | ArrayStep::ColumnSpacing => {
                let Some(distance) = number_in(text) else {
                    return not_understood(text, "a distance or a point");
                };
                if distance.abs() <= tolerance::GEOMETRIC {
                    return ToolStep::rejected(
                        "a spacing of zero would stack every copy on the original",
                    );
                }
                if now.step == ArrayStep::RowSpacing && now.columns > 1 {
                    self.state.remember();
                    self.state.now.row_spacing = distance;
                    self.state.now.step = ArrayStep::ColumnSpacing;
                    return ToolStep::next("");
                }
                if now.step == ArrayStep::RowSpacing {
                    self.finish(chosen, Vec2::new(0.0, distance))
                } else {
                    self.finish(chosen, Vec2::new(distance, now.row_spacing))
                }
            }
            ArrayStep::CellCorner | ArrayStep::ColumnSecond => not_understood(text, "a point"),
        }
    }

    fn step_point(&mut self, _cx: &ToolContext<'_>, chosen: &Chosen<'_>, at: Point2) -> ToolStep {
        let now = self.state.now;
        match now.step {
            ArrayStep::RowSpacing | ArrayStep::ColumnSpacing => {
                self.state.remember();
                self.state.now.first = at;
                self.state.now.step = if now.step == ArrayStep::RowSpacing {
                    ArrayStep::CellCorner
                } else {
                    ArrayStep::ColumnSecond
                };
                ToolStep::next("")
            }
            ArrayStep::CellCorner => {
                let cell = at - now.first;
                if now.rows > 1 && cell.y.abs() <= tolerance::GEOMETRIC {
                    return ToolStep::rejected(
                        "the unit cell has no height: the rows would stack on each other",
                    );
                }
                if now.columns > 1 && cell.x.abs() <= tolerance::GEOMETRIC {
                    return ToolStep::rejected(
                        "the unit cell has no width: the columns would stack on each other",
                    );
                }
                self.finish(chosen, cell)
            }
            ArrayStep::ColumnSecond => {
                // The distance between columns is measured across them, along
                // x; any rise between the two points is not part of it.
                let dx = at.x - now.first.x;
                if dx.abs() <= tolerance::GEOMETRIC {
                    return ToolStep::rejected(
                        "the points are level across the columns: a spacing of zero would stack \
                         them",
                    );
                }
                self.finish(chosen, Vec2::new(dx, now.row_spacing))
            }
            ArrayStep::Rows | ArrayStep::Columns => ToolStep::rejected("type a whole number"),
        }
    }

    fn step_enter(&mut self, _cx: &ToolContext<'_>, _chosen: &Chosen<'_>) -> ToolStep {
        let step = self.state.now.step;
        if step == ArrayStep::Rows || step == ArrayStep::Columns {
            return self.take_count(1); // <1>
        }
        ToolStep::rejected(format!("this step has no default: {}", self.step_prompt()))
    }

    fn step_undo(&mut self) -> bool {
        self.state.back()
    }

    fn step_preview(&self, chosen: &Chosen<'_>, cursor: Point2) -> ToolFeedback {
        let mut feedback = ToolFeedback::default();
        let now = self.state.now;
        if now.step == ArrayStep::CellCorner {
            self.add_array(&mut feedback, chosen, cursor - now.first);
            feedback.markers.push(now.first);
        } else if now.step == ArrayStep::ColumnSecond {
            self.add_array(
                &mut feedback,
                chosen,
                Vec2::new(cursor.x - now.first.x, now.row_spacing),
            );
            add_band(&mut feedback, now.first, cursor);
        }
        feedback
    }

    fn step_last_point(&self) -> Option<Point2> {
        let now = self.state.now;
        if now.step == ArrayStep::CellCorner || now.step == ArrayStep::ColumnSecond {
            return Some(now.first);
        }
        None
    }
}

// ---- Array: polar --------------------------------------------------------------------

// A centre, the number of items (the original is one of them), the angle to
// fill - 360 by default, counter-clockwise, negative for clockwise - and
// whether the items turn as they go round. A full circle spaces the items by
// 360/n, since the last would otherwise land on the first; a part circle puts
// the first and last items at its ends, spacing them by angle/(n - 1), as
// AutoCAD does.
//
// Items that do not turn are carried round by the middle of the selection's
// extent. (AutoCAD carries them by the base point of the last object selected,
// which a selection of several made in any order does not make obvious.)
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum PolarStep {
    #[default]
    Centre,
    Count,
    Fill,
    RotateItems,
}

#[derive(Clone, Copy, Debug)]
struct PolarState {
    step: PolarStep,
    centre: Point2,
    count: i32,
    /// Degrees.
    fill: f64,
}

impl Default for PolarState {
    fn default() -> Self {
        Self {
            step: PolarStep::Centre,
            centre: Point2::default(),
            count: 0,
            fill: 360.0,
        }
    }
}

#[derive(Default)]
struct ArrayPolarSteps {
    state: History<PolarState>,
}

impl ArrayPolarSteps {
    fn fill(&mut self, degrees: f64) -> ToolStep {
        if degrees.abs() <= tolerance::ANGULAR {
            return ToolStep::rejected(
                "an angle to fill of 0 would stack every item on the original",
            );
        }
        if degrees.abs() > 360.0 {
            return ToolStep::rejected("the angle to fill is at most 360 degrees either way");
        }
        self.state.remember();
        self.state.now.fill = degrees;
        self.state.now.step = PolarStep::RotateItems;
        ToolStep::next("")
    }

    fn middle_of_selection(chosen: &Chosen<'_>) -> Option<Point2> {
        let document = chosen.document?;
        let mut extent = Box2::default();
        for &id in chosen.ids {
            if let Some(entity) = document.model().entities.find(id) {
                extent.expand_box(bounding_box(&entity.geometry));
            }
        }
        if extent.empty() {
            return None;
        }
        Some(extent.center())
    }

    fn items(&self, chosen: &Chosen<'_>, fill_degrees: f64, rotate: bool) -> Vec<Mat3> {
        let now = self.state.now;
        let whole_turn = (fill_degrees.abs() - 360.0).abs() <= tolerance::ANGULAR;
        let step = fill_degrees * DEG_TO_RAD
            / f64::from(if whole_turn { now.count } else { now.count - 1 });
        let carried = Self::middle_of_selection(chosen).unwrap_or(now.centre);
        let mut out = Vec::new();
        for item in 1..now.count {
            let turn = Mat3::rotation_about(now.centre, step * f64::from(item));
            out.push(if rotate {
                turn
            } else {
                Mat3::translation(turn.transform_point(carried) - carried)
            });
        }
        out
    }

    fn finish(&self, chosen: &Chosen<'_>, rotate: bool) -> ToolStep {
        let now = self.state.now;
        ToolStep::done(
            Some(duplicate_entities(
                "ARRAY",
                chosen.ids.to_vec(),
                self.items(chosen, now.fill, rotate),
            )),
            format!(
                "polar array of {} items of {}",
                now.count,
                entities(chosen.ids.len())
            ),
            false,
        )
    }
}

impl TransformSteps for ArrayPolarSteps {
    fn step_prompt(&self) -> String {
        String::from(match self.state.now.step {
            PolarStep::Centre => "Specify center point of array",
            PolarStep::Count => "Enter the number of items in the array",
            PolarStep::Fill => "Specify the angle to fill (+=ccw, -=cw) <360>",
            PolarStep::RotateItems => "Rotate arrayed entities? [Yes/No] <Yes>",
        })
    }

    fn step_expects(&self) -> ToolInput {
        if self.state.now.step == PolarStep::Centre {
            ToolInput::Point
        } else {
            ToolInput::Value
        }
    }

    fn step_point(&mut self, _cx: &ToolContext<'_>, _chosen: &Chosen<'_>, at: Point2) -> ToolStep {
        if self.state.now.step != PolarStep::Centre {
            return ToolStep::rejected(format!("type the value: {}", self.step_prompt()));
        }
        self.state.remember();
        self.state.now.centre = at;
        self.state.now.step = PolarStep::Count;
        ToolStep::next("")
    }

    fn step_value(&mut self, _cx: &ToolContext<'_>, chosen: &Chosen<'_>, text: &str) -> ToolStep {
        match self.state.now.step {
            PolarStep::Count => {
                let Some(count) = integer_in(text).filter(|&count| count >= 2) else {
                    return ToolStep::rejected(
                        "the number of items counts the original, so it is a whole number, 2 or \
                         more",
                    );
                };
                if count > MAXIMUM_ARRAY_ITEMS {
                    return ToolStep::rejected("an array is at most 100000 items");
                }
                self.state.remember();
                // At most MAXIMUM_ARRAY_ITEMS, so it fits.
                self.state.now.count = count as i32;
                self.state.now.step = PolarStep::Fill;
                ToolStep::next("")
            }
            PolarStep::Fill => {
                let Some(degrees) = number_in(text) else {
                    return not_understood(text, "an angle in degrees");
                };
                self.fill(degrees)
            }
            PolarStep::RotateItems => {
                if is_option(text, "Y", "YES") {
                    return self.finish(chosen, true);
                }
                if is_option(text, "N", "NO") {
                    return self.finish(chosen, false);
                }
                not_understood(text, "an answer: type Yes or No")
            }
            PolarStep::Centre => not_understood(text, "a point"),
        }
    }

    fn step_enter(&mut self, _cx: &ToolContext<'_>, chosen: &Chosen<'_>) -> ToolStep {
        match self.state.now.step {
            PolarStep::Fill => self.fill(360.0),
            PolarStep::RotateItems => self.finish(chosen, true),
            PolarStep::Centre | PolarStep::Count => {
                ToolStep::rejected(format!("this step has no default: {}", self.step_prompt()))
            }
        }
    }

    fn step_undo(&mut self) -> bool {
        self.state.back()
    }

    fn step_preview(&self, chosen: &Chosen<'_>, cursor: Point2) -> ToolFeedback {
        let mut feedback = ToolFeedback::default();
        let now = self.state.now;
        match now.step {
            PolarStep::Centre => {}
            PolarStep::Count => add_band(&mut feedback, now.centre, cursor),
            PolarStep::Fill | PolarStep::RotateItems => {
                // What Enter would make: the angle's default, turning the
                // items. As the rectangular array: not built only to be
                // declined. The count is 2 or more here.
                feedback.markers.push(now.centre);
                if (now.count - 1) as usize * chosen.ids.len() <= PREVIEW_LIMIT {
                    let transforms = self.items(chosen, now.fill, true);
                    add_transformed(&mut feedback, chosen.document, chosen.ids, &transforms);
                }
            }
        }
        feedback
    }

    fn step_last_point(&self) -> Option<Point2> {
        let now = self.state.now;
        if now.step == PolarStep::Centre {
            return None;
        }
        Some(now.centre)
    }
}

// ---- Erase ---------------------------------------------------------------------------

struct EraseSteps;

impl TransformSteps for EraseSteps {
    fn step_prompt(&self) -> String {
        String::new()
    }

    fn selected(&mut self, chosen: &Chosen<'_>) -> ToolStep {
        ToolStep::done(
            Some(delete_entities(chosen.ids.to_vec())),
            format!("{} erased", entities(chosen.ids.len())),
            false,
        )
    }
}

// ---- Stretch -------------------------------------------------------------------------
//
// A crossing window, then a displacement. What lies wholly inside the window
// moves; what crosses it has only the points inside moved - a line's ends, a
// polyline's vertices, a dimension's definition points. A circle, a point and
// a text move when their centre, position or insertion point is inside. An
// arc with one end inside keeps its chord height (the sagitta) and is rebuilt
// through its new ends, as AutoCAD stretches one.
//
// This is expressible without a new command: the stretch is one change set
// modifying each entity it touches, built from the model when it executes.

// The geometry with its defining points inside `window` moved by `delta`, or
// None when none is inside and it would not change.
#[derive(Clone, Copy, Debug)]
struct Stretcher {
    window: Box2,
    delta: Vec2,
}

impl Stretcher {
    fn moved(&self, p: Point2) -> Point2 {
        if self.window.contains(p) {
            p + self.delta
        } else {
            p
        }
    }

    fn inside(&self, p: Point2) -> bool {
        self.window.contains(p)
    }

    fn stretch(&self, geometry: &Geometry) -> Result<Option<Geometry>> {
        match geometry {
            Geometry::Point(point) => {
                if !self.inside(point.position) {
                    return Ok(None);
                }
                Ok(Some(Geometry::Point(PointGeometry {
                    position: point.position + self.delta,
                })))
            }
            Geometry::Line(segment) => {
                if !self.inside(segment.start) && !self.inside(segment.end) {
                    return Ok(None);
                }
                Ok(Some(Geometry::Line(Segment2::new(
                    self.moved(segment.start),
                    self.moved(segment.end),
                ))))
            }
            Geometry::Arc(arc) => self.arc(arc),
            Geometry::Polyline(polyline) => {
                if !polyline.vertices.iter().any(|&p| self.inside(p)) {
                    return Ok(None);
                }
                let mut out: Polyline2 = polyline.clone();
                for vertex in &mut out.vertices {
                    *vertex = self.moved(*vertex);
                }
                Ok(Some(Geometry::Polyline(out)))
            }
            Geometry::Circle(circle) => {
                if !self.inside(circle.center) {
                    return Ok(None);
                }
                Ok(Some(Geometry::Circle(Circle2 {
                    center: circle.center + self.delta,
                    radius: circle.radius,
                })))
            }
            Geometry::Text(text) => {
                if !self.inside(text.position) {
                    return Ok(None);
                }
                let mut out = text.clone();
                out.position = text.position + self.delta;
                Ok(Some(Geometry::Text(out)))
            }
            Geometry::Dimension(dimension) => {
                if !self.inside(dimension.start) && !self.inside(dimension.end) {
                    return Ok(None);
                }
                let mut out = dimension.clone();
                out.start = self.moved(dimension.start);
                out.end = self.moved(dimension.end);
                if dimension.uses_vertex() {
                    out.vertex = self.moved(dimension.vertex);
                }
                let out = Geometry::Dimension(out);
                if validate(&out).is_err() {
                    return Err(Error::new(
                        ErrorCode::InvalidArgument,
                        "stretching would bring the dimension's points together",
                    ));
                }
                Ok(Some(out))
            }
            // A dragged label's place moves when it is inside; where a label
            // attaches is its target's business (the associative update).
            Geometry::Label(label) => {
                let Some(position) = label.position.filter(|&position| self.inside(position))
                else {
                    return Ok(None);
                };
                let mut out = label.clone();
                out.position = Some(position + self.delta);
                Ok(Some(Geometry::Label(out)))
            }
            // A leader's vertices inside the window move, as a polyline's do.
            Geometry::Leader(leader) => {
                if !leader.vertices.iter().any(|&p| self.inside(p)) {
                    return Ok(None);
                }
                let mut out = leader.clone();
                for vertex in &mut out.vertices {
                    *vertex = self.moved(*vertex);
                }
                let out = Geometry::Leader(out);
                if validate(&out).is_err() {
                    return Err(Error::new(
                        ErrorCode::InvalidArgument,
                        "stretching would bring the leader's points together",
                    ));
                }
                Ok(Some(out))
            }
            // A curve polyline's vertices inside the window move and each arc
            // keeps its bulge, so it keeps its shape between its ends.
            Geometry::CurvePolyline(polyline) => {
                if !polyline
                    .vertices
                    .iter()
                    .any(|vertex| self.inside(vertex.position))
                {
                    return Ok(None);
                }
                let mut out = polyline.clone();
                for vertex in &mut out.vertices {
                    vertex.position = self.moved(vertex.position);
                }
                let out = Geometry::CurvePolyline(out);
                if validate(&out).is_err() {
                    return Err(Error::new(
                        ErrorCode::InvalidArgument,
                        "stretching would collapse the polyline",
                    ));
                }
                Ok(Some(out))
            }
            // An ellipse moves whole when its centre is inside, as a circle
            // does.
            Geometry::Ellipse(ellipse) => {
                if !self.inside(ellipse.center) {
                    return Ok(None);
                }
                let mut out = *ellipse;
                out.center = ellipse.center + self.delta;
                Ok(Some(Geometry::Ellipse(out)))
            }
            // A spline's defining points inside the window move - its fit
            // points, through which it is solved again, or its control points.
            Geometry::Spline(spline) => {
                let points = if spline.has_fit_points() {
                    &spline.fit_points
                } else {
                    &spline.control_points
                };
                if !points.iter().any(|&p| self.inside(p)) {
                    return Ok(None);
                }
                let shifted: Vec<Point2> = points.iter().map(|&p| self.moved(p)).collect();
                if !spline.has_fit_points() {
                    let mut out = spline.clone();
                    out.control_points = shifted;
                    return Ok(Some(Geometry::Spline(out)));
                }
                let out = Spline2::through_points(shifted, spline.degree)?;
                Ok(Some(Geometry::Spline(out)))
            }
        }
    }

    fn arc(&self, arc: &Arc2) -> Result<Option<Geometry>> {
        let start = arc.start_point();
        let end = arc.end_point();
        let start_inside = self.inside(start);
        let end_inside = self.inside(end);
        if !start_inside && !end_inside {
            return Ok(None);
        }
        if start_inside && end_inside {
            return Ok(Some(Geometry::Arc(Arc2 {
                center: arc.center + self.delta,
                ..*arc
            })));
        }
        // One end moves. The height of the arc above its chord, signed by the
        // side it bulges to, is kept, and the arc is the one through the new
        // ends and the point that height above the new chord's middle.
        let chord = end - start;
        let height = chord.normalized().cross(arc.midpoint() - start);
        let new_start = self.moved(start);
        let new_end = self.moved(end);
        let new_chord = new_end - new_start;
        if new_chord.length() <= tolerance::GEOMETRIC {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "stretching would bring the arc's ends together",
            ));
        }
        let middle = (new_start + new_end) * 0.5 + new_chord.normalized().perpendicular() * height;
        let Some(rebuilt) = Arc2::through_points(new_start, middle, new_end) else {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "the stretched arc would be degenerate",
            ));
        };
        Ok(Some(Geometry::Arc(rebuilt)))
    }
}

// The change set of a stretch: every entity of `ids` the window touches.
fn stretch_changes(
    model: &Model,
    ids: &[EntityId],
    window: Box2,
    delta: Vec2,
) -> Result<ChangeSet> {
    let mut changes = ChangeSet::default();
    let stretcher = Stretcher { window, delta };
    for &id in ids {
        let Some(entity) = model.entities.find(id) else {
            return Err(make_error(
                ErrorCode::NotFound,
                "entity does not exist",
                format!("id={id}"),
            ));
        };
        let stretched = stretcher
            .stretch(&entity.geometry)
            .map_err(|error| make_error(error.code, error.message, format!("id={id}")))?;
        if let Some(geometry) = stretched {
            let mut changed = entity.clone();
            changed.geometry = geometry;
            changes.modify.push(changed);
        }
    }
    if changes.empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "nothing in the window has a point inside it to stretch",
        ));
    }
    Ok(changes)
}

fn stretch_command(ids: Vec<EntityId>, window: Box2, delta: Vec2) -> CommandPtr {
    Box::new(ChangeSetCommand::new(
        "STRETCH",
        move |context| stretch_changes(&*context.model, &ids, window, delta),
        false,
    ))
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum StretchStep {
    #[default]
    FirstCorner,
    OppositeCorner,
    Base,
    Typed,
    Second,
}

#[derive(Clone, Debug, Default)]
struct StretchState {
    step: StretchStep,
    corner: Point2,
    window: Box2,
    /// What the window touches.
    caught: Vec<EntityId>,
    base: Point2,
}

// The window's outline as a closed polyline: `bounds` grown to take in `to`.
fn outline(mut bounds: Box2, to: Point2) -> Polyline2 {
    bounds.expand(to);
    Polyline2::new(
        vec![
            bounds.min,
            Point2::new(bounds.max.x, bounds.min.y),
            bounds.max,
            Point2::new(bounds.min.x, bounds.max.y),
        ],
        true,
    )
}

struct StretchTool {
    /// Ascending, as ToolStart gives it.
    limit: Vec<EntityId>,
    state: History<StretchState>,
    cursor: Cell<Option<Point2>>,
}

impl StretchTool {
    // The window is the selection, so Stretch never asks for one; a selection
    // made beforehand limits what the window can catch.
    fn new(start: &ToolStart) -> Self {
        Self {
            limit: start.selection.clone(),
            state: History::default(),
            cursor: Cell::new(None),
        }
    }

    fn window(&mut self, cx: &ToolContext<'_>, opposite: Point2) -> ToolStep {
        let corner = self.state.now.corner;
        let mut bounds = Box2::new(corner, corner);
        bounds.expand(opposite);
        if bounds.width() <= tolerance::GEOMETRIC || bounds.height() <= tolerance::GEOMETRIC {
            return ToolStep::rejected("the window has no area: pick a corner diagonally opposite");
        }
        let Some(document) = cx.document else {
            return ToolStep::rejected("there is no drawing to stretch");
        };
        let model = document.model();
        // No filter view: the C++ passes none, as it gives All none (see the
        // All option of SelectionTool).
        let mut caught = pick_in_box(
            model,
            bounds,
            BoxSelectionMode::Crossing,
            &SelectionFilter::default(),
            Some(document.spatial_index()),
        );
        if !self.limit.is_empty() {
            caught.retain(|id| self.limit.binary_search(id).is_ok());
        }
        // Only what has a point inside can stretch: a line merely crossing the
        // window has neither end in it, and stays as it is.
        let probe = Stretcher {
            window: bounds,
            delta: Vec2::new(1.0, 0.0),
        };
        caught.retain(|&id| {
            let Some(entity) = model.entities.find(id) else {
                return false;
            };
            !matches!(probe.stretch(&entity.geometry), Ok(None))
        });
        if caught.is_empty() {
            return ToolStep::rejected(
                "nothing has a point inside the window: a line or polyline stretches by the ends \
                 or vertices the window contains",
            );
        }
        self.state.remember();
        let count = caught.len();
        self.state.now.window = bounds;
        self.state.now.caught = caught;
        self.state.now.step = StretchStep::Base;
        ToolStep::next(format!("{} to stretch", entities(count)))
    }

    fn finish(&self, cx: &ToolContext<'_>, delta: Vec2) -> ToolStep {
        if delta.length() <= tolerance::GEOMETRIC {
            return ToolStep::rejected("a displacement of zero would stretch nothing");
        }
        let now = &self.state.now;
        // Built once here, and checked as the document will check it, to
        // refuse with its reason a stretch the document would refuse. The
        // Stretcher refuses an arc or a dimension whose ends would meet, but a
        // line or polyline stretched onto itself is only caught by the model's
        // own rule (zero length). Done would end the tool and lose the window
        // and base point for a command that then fails.
        if let Some(document) = cx.document {
            let changes = match stretch_changes(document.model(), &now.caught, now.window, delta) {
                Ok(changes) => changes,
                Err(error) => return ToolStep::rejected(error.message),
            };
            if let Err(error) = validate_change_set(&changes, document.model()) {
                return ToolStep::rejected(format!(
                    "that stretch cannot be made: {}",
                    error.message
                ));
            }
        }
        ToolStep::done(
            Some(stretch_command(now.caught.clone(), now.window, delta)),
            format!("{} stretched", entities(now.caught.len())),
            false,
        )
    }
}

impl InteractiveTool for StretchTool {
    fn prompt(&self, _cx: &ToolContext<'_>) -> String {
        match self.state.now.step {
            StretchStep::FirstCorner => String::from("Specify first corner of the crossing window"),
            StretchStep::OppositeCorner => String::from("Specify opposite corner"),
            StretchStep::Base => displacement_prompt(Displace::Base),
            StretchStep::Typed => displacement_prompt(Displace::Typed),
            StretchStep::Second => displacement_prompt(Displace::Second),
        }
    }

    fn expects(&self, _cx: &ToolContext<'_>) -> ToolInput {
        ToolInput::Point
    }

    fn point(&mut self, cx: &ToolContext<'_>, at: Point2) -> ToolStep {
        match self.state.now.step {
            StretchStep::FirstCorner => {
                self.state.remember();
                self.state.now.corner = at;
                self.state.now.step = StretchStep::OppositeCorner;
                ToolStep::next("")
            }
            StretchStep::OppositeCorner => self.window(cx, at),
            StretchStep::Base => {
                self.state.remember();
                self.state.now.base = at;
                self.state.now.step = StretchStep::Second;
                ToolStep::next("")
            }
            StretchStep::Typed => self.finish(cx, at - ORIGIN),
            StretchStep::Second => self.finish(cx, at - self.state.now.base),
        }
    }

    fn value(&mut self, cx: &ToolContext<'_>, text: &str) -> ToolStep {
        let step = self.state.now.step;
        if step == StretchStep::Base && is_option(text, "D", "DISPLACEMENT") {
            self.state.remember();
            self.state.now.step = StretchStep::Typed;
            return ToolStep::next("");
        }
        if step == StretchStep::Second
            && let Some(distance) = number_in(text)
        {
            let base = self.state.now.base;
            let Some(to) = along_cursor(distance, base, self.cursor.get()) else {
                return ToolStep::rejected(NO_DIRECTION);
            };
            return self.finish(cx, to - base);
        }
        not_understood(text, "a point, a distance or an option here")
    }

    fn enter(&mut self, cx: &ToolContext<'_>) -> ToolStep {
        match self.state.now.step {
            StretchStep::Base => {
                self.state.remember();
                self.state.now.step = StretchStep::Typed;
                ToolStep::next("")
            }
            StretchStep::Second => self.finish(cx, self.state.now.base - ORIGIN),
            StretchStep::Typed => ToolStep::rejected("type the displacement as dx,dy"),
            StretchStep::FirstCorner | StretchStep::OppositeCorner => {
                ToolStep::rejected("pick the corners of the crossing window first")
            }
        }
    }

    fn undo(&mut self, _cx: &ToolContext<'_>) -> ToolStep {
        if self.state.back() {
            ToolStep::next("")
        } else {
            ToolStep::rejected("nothing to undo in this tool")
        }
    }

    fn preview(&self, cx: &ToolContext<'_>, cursor: Point2) -> ToolFeedback {
        self.cursor.set(Some(cursor));
        let mut feedback = ToolFeedback::default();
        let now = &self.state.now;
        match now.step {
            StretchStep::FirstCorner => {}
            StretchStep::OppositeCorner => {
                feedback.shapes.push(Geometry::Polyline(outline(
                    Box2::new(now.corner, now.corner),
                    cursor,
                )));
                feedback.markers.push(now.corner);
            }
            StretchStep::Base | StretchStep::Typed => {
                feedback
                    .shapes
                    .push(Geometry::Polyline(outline(now.window, now.window.max)));
            }
            StretchStep::Second => {
                feedback
                    .shapes
                    .push(Geometry::Polyline(outline(now.window, now.window.max)));
                if let Some(document) = cx.document
                    && now.caught.len() <= PREVIEW_LIMIT
                    && !coincident(cursor, now.base)
                    && let Ok(changes) = stretch_changes(
                        document.model(),
                        &now.caught,
                        now.window,
                        cursor - now.base,
                    )
                {
                    for entity in changes.modify {
                        feedback.shapes.push(entity.geometry);
                    }
                }
                add_band(&mut feedback, now.base, cursor);
            }
        }
        feedback
    }

    fn last_point(&self, _cx: &ToolContext<'_>) -> Option<Point2> {
        let now = &self.state.now;
        match now.step {
            StretchStep::FirstCorner | StretchStep::Base => None,
            StretchStep::OppositeCorner => Some(now.corner),
            StretchStep::Typed => Some(ORIGIN),
            StretchStep::Second => Some(now.base),
        }
    }
}

// ---- the catalogue -------------------------------------------------------------------

fn transform_tool(
    id: &str,
    name: &str,
    order: i32,
    aliases: &[&str],
    tip: &str,
    make: ToolFactory,
) -> ToolInfo {
    ToolInfo {
        id: String::from(id),
        name: String::from(name),
        category: String::from("Modify"),
        group: String::from("Transform"),
        order,
        aliases: aliases.iter().map(|&alias| String::from(alias)).collect(),
        tip: String::from(tip),
        make: Some(make),
        ..ToolInfo::default()
    }
}

// A transform tool's factory: its steps behind the selection it asks for.
fn selection_tool<Steps>(verb: &'static str, confirms: bool, steps: fn() -> Steps) -> ToolFactory
where
    Steps: TransformSteps + 'static,
{
    factory(move |_, start| -> Box<dyn InteractiveTool> {
        Box::new(SelectionTool::new(start, verb, confirms, steps()))
    })
}

/// Move, Copy, Rotate, Scale, Mirror, Stretch, Array, Erase (the C++
/// addModifyTransformTools).
pub fn add_modify_transform_tools(catalog: &mut ToolCatalog, report: &mut Report<'_>) {
    report(catalog.add(transform_tool(
        "modify.move",
        "Move",
        10,
        &["MOVE", "M"],
        "Moves the selection from a base point to a second point, or by a typed displacement.",
        selection_tool("move", false, MoveSteps::default),
    )));
    report(catalog.add(transform_tool(
        "modify.copy",
        "Copy",
        20,
        &["COPY", "CO", "CP"],
        "Copies the selection from a base point to as many second points as you pick; Enter \
         ends.",
        selection_tool("copy", false, CopySteps::default),
    )));
    report(catalog.add(transform_tool(
        "modify.rotate",
        "Rotate",
        30,
        &["ROTATE", "RO"],
        "Rotates the selection about a base point by a typed or picked angle, or from a \
         reference angle to a new one.",
        selection_tool("rotate", false, RotateSteps::default),
    )));
    report(catalog.add(transform_tool(
        "modify.scale",
        "Scale",
        40,
        &["SCALE", "SC"],
        "Scales the selection about a base point by a factor, or from a reference length to a \
         new one.",
        selection_tool("scale", false, ScaleSteps::default),
    )));
    report(catalog.add(transform_tool(
        "modify.mirror",
        "Mirror",
        50,
        &["MIRROR", "MI"],
        "Mirrors the selection about a line through two points, keeping or erasing the source.",
        selection_tool("mirror", false, MirrorSteps::default),
    )));
    report(catalog.add(transform_tool(
        "modify.stretch",
        "Stretch",
        60,
        &["STRETCH", "S"],
        "Moves the ends and vertices inside a crossing window, stretching what crosses it and \
         moving what lies within.",
        factory(|_, start| -> Box<dyn InteractiveTool> { Box::new(StretchTool::new(start)) }),
    )));
    report(catalog.add(transform_tool(
        "modify.array_rectangular",
        "Rectangular Array",
        70,
        &["ARRAYRECT", "ARRAY", "AR"],
        "Copies the selection into rows and columns at a typed spacing or a picked unit cell.",
        selection_tool("array", false, ArrayRectangularSteps::default),
    )));
    report(catalog.add(transform_tool(
        "modify.array_polar",
        "Polar Array",
        80,
        &["ARRAYPOLAR"],
        "Copies the selection round a centre point: a number of items over an angle, turning \
         them or not.",
        selection_tool("array", false, ArrayPolarSteps::default),
    )));
    report(catalog.add(transform_tool(
        "modify.erase",
        "Erase",
        90,
        &["ERASE", "E", "DELETE", "DEL"],
        "Erases the selection.",
        selection_tool("erase", true, || EraseSteps),
    )));
}
