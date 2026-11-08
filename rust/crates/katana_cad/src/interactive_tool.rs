//! Interactive tools: the drawing, editing, annotation and inquiry tools the
//! plan view runs (include/katana/cad/interactive_tool.hpp; the owner's
//! request of 2026-09-23: "more CAD tools ... make it professional CAD
//! software").
//!
//! A tool is a STATE MACHINE, and it lives here in cad rather than in the
//! view. The plan view tells it what the user did - a point picked (already
//! snapped), an entity picked, a value typed, Enter, Undo - and the tool
//! answers with the next prompt, a rubber-band preview for the cursor and,
//! when it completes, ONE command for the document to execute. So every tool
//! is unit tested by feeding it a sequence of inputs and comparing the
//! entities its command produces with geometry worked out by hand, with no
//! window anywhere.
//!
//! Tools are listed in a catalogue (ToolCatalog) that the menus, toolbars,
//! command-line aliases and status tips are all generated from, so a tool is
//! added by writing it and adding it to its family's list - never by editing
//! the window. The program's catalogue is built from the family list in
//! tools/families.rs ([`crate::tools::families::FAMILIES`]).
//!
//! # The Rust shape of a tool, and why (the port's cad plan, section 7, risk 2)
//!
//! The C++ tool keeps `const Document*` from its ToolContext for its whole
//! life, and the host executes each finished step's command on that same
//! document between one input and the next. A Rust tool cannot keep `&Document`
//! while the host takes `&mut Document` to execute, so the context is split in
//! two, by how long each part lives:
//!
//! * **[`ToolStart`] is what a tool is MADE with**, by value: the attributes
//!   for what it creates, the selection when it started, and the grips hot in
//!   the view (`handles`, given to the first make only). The factory
//!   ([`ToolInfo::make`]) is handed it once; a tool keeps what it needs, as
//!   the C++ constructors copy `context.attributes` and `context.selection`.
//! * **[`ToolContext`] is what EVERY call is LENT**: the document as it is
//!   now, the view's pick and apertures now, the view's hidden layers, and
//!   the pick tolerance the tool was made with. It is `Copy`, it is built by
//!   the host for each call and dropped before the host executes anything,
//!   and every method of [`InteractiveTool`] takes it - `prompt` and
//!   `expects` too, so a tool never has to wonder which methods may read the
//!   document. The host gives every call the context it made the tool with,
//!   only the document and the view's answers being current; the factory is
//!   given the same context besides its ToolStart.
//!
//! A tool therefore holds no reference at all: it is `'static`, owned by its
//! host as `Box<dyn InteractiveTool>`. Porting a C++ tool is mechanical:
//! `document_` becomes `cx.document` and `context_` becomes `cx` (the first
//! parameter after `self` of every method), and a member copied from the
//! context at construction is copied from the ToolStart. A helper that held
//! `const Document*` (the everyday families' SelectionStep, modify_vertex's
//! collected picks) takes `Option<&Document>` per call instead of keeping it.
//! A test reaches a tool's own state through [`InteractiveTool::as_any_mut`],
//! where the C++ used `dynamic_cast`.
//!
//! Rejected: one struct with the C++'s fields, rebuilt by the host for every
//! call (the selection, attributes and handles cloned on every mouse move,
//! and nothing to say which fields a later call may trust); and a tool that
//! keeps an `Rc`/`Arc` of a shared, interior-mutable Document, which would
//! make the Document shareable for the sake of one front end and put every
//! command's execution behind a run-time borrow.
//!
//! **The drafting aids' settings** ([`route_typed_input_with_drafting`]) are
//! the host's copy: the router takes `&mut DraftingSettings` that is NOT the
//! document's, since the context lends the document to the tool for the
//! call. DraftingSettings is `Copy`: the host copies it out
//! (`*document.drafting()`), routes, and writes it back
//! (`*document.drafting_mut() = drafting`) once the call has returned.
//! Rejected: a router taking `&mut Document`, which would have to lend the
//! tool an immutable document while holding a mutable one, and would tie the
//! router to a host that owns the document outright.
//!
//! The C++'s null CommandPtr in a ToolStep is `Option<CommandPtr>`; its nested
//! `ToolStep::Outcome` is [`ToolStepOutcome`].

use std::any::Any;
use std::collections::BTreeSet;
use std::fmt;
use std::sync::{Arc, OnceLock};

use katana_commands::{CommandPtr, EntityAttributes};
use katana_core::error::make_error;
use katana_core::text::{equals_ignoring_case, format_exact_real, parse_finite_double};
use katana_core::{ErrorCode, Result, Status};
use katana_entity::entity::to_string;
use katana_entity::{AnchorRef, Entity, EntityId, EntityType, EnumName};
use katana_geometry::Point2;
use katana_math::numerics::RAD_TO_DEG;

use crate::document::Document;
use crate::drawing::drafting::{
    DraftingSettings, PrecisePoint, direct_distance, format_bearing, format_dms, looks_like_point,
    parse_direction, parse_precise_point,
};
use crate::drawing::grips::Grip;
use crate::layer_overrides::{LayerOverrides, NO_LAYER_OVERRIDES};
use crate::selection::{SelectionFilter, is_selectable, pick_entity};
use crate::snapping::{SnapResult, snap_anchor};
use crate::tool_feedback::ToolFeedback;
use crate::tools::families::FAMILIES;

/// What the tool wants next. The view uses it to decide what a click means,
/// which cursor to show and whether snapping applies.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ToolInput {
    /// A click is a point; snapping applies.
    #[default]
    Point,
    /// A click picks the entity under the cursor (Trim's edges, Offset's
    /// source).
    Entity,
    /// A click selects as the Select tool does; Enter ends the selection.
    Selection,
    /// Only typed input is meaningful (a count, a text string).
    Value,
}

/// The C++ `toString(ToolInput)`.
impl EnumName for ToolInput {
    fn name(self) -> &'static str {
        match self {
            Self::Point => "point",
            Self::Entity => "entity",
            Self::Selection => "selection",
            Self::Value => "value",
        }
    }
}

/// The C++ `ToolStep::Outcome`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ToolStepOutcome {
    /// Input accepted; more is needed (see prompt()).
    #[default]
    Continue,
    /// The tool has finished; execute `command` if there is one.
    Done,
    /// Input refused; `message` says why, and the tool is unchanged.
    Rejected,
}

/// The tool's answer to one input.
#[derive(Default)]
pub struct ToolStep {
    pub outcome: ToolStepOutcome,
    /// Done: what to execute - ONE command, so one undo removes the whole
    /// operation. None for a tool that only reports (Distance, List) or that
    /// was finished with nothing to do.
    pub command: Option<CommandPtr>,
    /// Rejected: why. Otherwise an optional line for the command log - a
    /// measurement, or what was made ("3 lines").
    pub message: String,
    /// Done: start the tool again from its first step, as a CAD user expects
    /// of Circle or Text - press Esc to stop. False for tools whose next use
    /// needs a fresh selection (Move, Rotate).
    pub restart: bool,
    /// Done: the selection to leave behind, for a tool whose answer IS a
    /// selection (Select Similar, Quick Select). The host puts it in the
    /// document's selection set when the step's command, if any, has run;
    /// None leaves the selection as it is. A selection is not a command - it
    /// is not undone with the drawing - so it travels beside one.
    pub selection: Option<Vec<EntityId>>,
}

impl fmt::Debug for ToolStep {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ToolStep")
            .field("outcome", &self.outcome)
            .field(
                "command",
                &self.command.as_ref().map(|command| command.name()),
            )
            .field("message", &self.message)
            .field("restart", &self.restart)
            .field("selection", &self.selection)
            .finish()
    }
}

impl ToolStep {
    /// Continue, with an optional line for the log (the C++ default is "").
    pub fn next(message: impl Into<String>) -> Self {
        Self {
            outcome: ToolStepOutcome::Continue,
            message: message.into(),
            ..Self::default()
        }
    }

    pub fn rejected(why: impl Into<String>) -> Self {
        Self {
            outcome: ToolStepOutcome::Rejected,
            message: why.into(),
            ..Self::default()
        }
    }

    /// Done (the C++ defaults are an empty message and no restart).
    pub fn done(command: Option<CommandPtr>, message: impl Into<String>, restart: bool) -> Self {
        Self {
            outcome: ToolStepOutcome::Done,
            command,
            message: message.into(),
            restart,
            selection: None,
        }
    }
}

// The rubber band (ToolFeedback) is in tool_feedback.rs: the ghost of the
// result, the base points, and marks by role - what a click takes, adds and
// removes.

/// The plan view's two apertures in pixels, here where the tools that measure
/// in them can see them: a click picks what lies within 8 px, and a snap - and
/// a vertex tool's pick of a vertex, the reach an Endpoint snap has - within
/// 12 px. The view converts them at its zoom (ToolContext::pick_aperture and
/// vertex_aperture); a tool that needs one pixel divides by them. They were
/// the view's own constants, restated as bare 8s and 12s in the tools.
pub const PICK_APERTURE_PIXELS: f64 = 8.0;
pub const SNAP_APERTURE_PIXELS: f64 = 12.0;

/// The view's own pick: the entity of one of `types` (any, when empty)
/// nearest `at` within `reach` model units that the VIEW lets be picked -
/// through its hidden layers - or None. What a click there gives entity().
pub type ViewPick<'a> = dyn Fn(Point2, f64, &BTreeSet<EntityType>) -> Option<EntityId> + 'a;

/// One of the view's apertures, read now, in model units.
pub type Aperture<'a> = dyn Fn() -> f64 + 'a;

/// What a tool is lent for one call - and for its make - by its host: the
/// document as it is now, and the view's pick, apertures and hidden layers
/// (module doc, "The Rust shape of a tool"). Everything is shared: a tool
/// never changes the document itself, it returns a command that does.
#[derive(Clone, Copy, Default)]
pub struct ToolContext<'a> {
    /// None in a test that needs no drawing.
    pub document: Option<&'a Document>,
    /// The view's pick aperture in model units when the tool was made (at
    /// its start or restart), for a tool that finds geometry near a point
    /// itself. A tool that picks as the cursor moves reads pick_reach()
    /// instead, which follows a zoom inside the tool.
    pub pick_tolerance: f64,
    /// The view's pick and apertures NOW (None in a test: pick_under,
    /// pick_reach and vertex_reach then fall back to the model and to
    /// pick_tolerance). One pick for the preview and the click, so what a
    /// preview promises is what the click takes.
    pub pick: Option<&'a ViewPick<'a>>,
    /// The view's 8 px, in model units.
    pub pick_aperture: Option<&'a Aperture<'a>>,
    /// The view's 12 px (its snap aperture).
    pub vertex_aperture: Option<&'a Aperture<'a>>,
    /// The layers the view hides of its own (LayerOverrides), for a tool that
    /// finds what to act on itself rather than through `pick`: what it takes
    /// must pass THE visibility rule there (pickable), as the view's pick
    /// does. None in a test: the document's rule alone.
    pub view: Option<&'a LayerOverrides>,
}

impl fmt::Debug for ToolContext<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ToolContext")
            .field("document", &self.document.is_some())
            .field("pick_tolerance", &self.pick_tolerance)
            .field("pick", &self.pick.is_some())
            .field("pick_aperture", &self.pick_aperture.is_some())
            .field("vertex_aperture", &self.vertex_aperture.is_some())
            .field("view", &self.view)
            .finish()
    }
}

/// What a tool is made with, once (module doc, "The Rust shape of a tool").
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ToolStart {
    /// For anything the tool creates: the current layer, style and colour.
    pub attributes: EntityAttributes,
    /// The selection when the tool started, in ascending id order. Transform
    /// and edit tools act on it; when it is empty they begin by asking for
    /// one.
    pub selection: Vec<EntityId>,
    /// The grips hot in the view when the tool started: a vertex or a segment
    /// the user chose BEFORE choosing the tool (docs/drawing.md, "What a
    /// vertex tool acts on"). Given to the first make only - a restarted tool
    /// never sees them, since the edit it follows may have renumbered the
    /// vertices they name.
    pub handles: Vec<Grip>,
}

/// The entity of one of `types` nearest `at` within `reach` (pick_reach when
/// None, the C++ default): through the view's pick when the context has one,
/// else through the model and its spatial index, with the context's view's
/// hidden layers.
pub fn pick_under(
    cx: &ToolContext<'_>,
    at: Point2,
    types: &BTreeSet<EntityType>,
    reach: Option<f64>,
) -> Option<EntityId> {
    let within = reach.unwrap_or_else(|| pick_reach(cx));
    if let Some(pick) = cx.pick {
        return pick(at, within, types);
    }
    let document = cx.document?;
    let filter = SelectionFilter {
        types: types.clone(),
        view: cx.view,
        ..SelectionFilter::default()
    };
    pick_entity(
        document.model(),
        at,
        within,
        &filter,
        Some(document.spatial_index()),
    )
}

/// The view's own hidden layers, or NO_LAYER_OVERRIDES when the context has
/// no view.
pub fn view_of<'a>(cx: &ToolContext<'a>) -> &'a LayerOverrides {
    cx.view.unwrap_or(&NO_LAYER_OVERRIDES)
}

/// Whether a tool may take `entity`: THE visibility rule (selection.rs,
/// is_selectable) in the tool's view - drawn there and not locked - the rule
/// every picker asks. A tool that looks through the selection or the spatial
/// index itself asks this, or it would edit what the user cannot see.
pub fn pickable(cx: &ToolContext<'_>, entity: &Entity) -> bool {
    cx.document
        .is_some_and(|document| is_selectable(document.model(), entity, view_of(cx)))
}

/// How near a pick of a piece must be: the view's pick aperture now, else
/// pick_tolerance.
pub fn pick_reach(cx: &ToolContext<'_>) -> f64 {
    match cx.pick_aperture {
        Some(aperture) => aperture(),
        None => cx.pick_tolerance,
    }
}

/// How near a pick of a VERTEX must be: the view's snap aperture now (the
/// reach an Endpoint snap has), else pick_tolerance in the proportion of the
/// view's two apertures (SNAP_APERTURE_PIXELS : PICK_APERTURE_PIXELS).
pub fn vertex_reach(cx: &ToolContext<'_>) -> f64 {
    match cx.vertex_aperture {
        Some(aperture) => aperture(),
        // With no view (a test), the tolerance stands for the pick aperture
        // and the vertex reach is in the view's proportion to it.
        None => SNAP_APERTURE_PIXELS / PICK_APERTURE_PIXELS * cx.pick_tolerance,
    }
}

/// A tool. Every method is lent the context of the call (module doc); the
/// defaults reject the input with a sentence saying what is expected instead,
/// so a tool implements only the inputs it takes.
pub trait InteractiveTool {
    /// The prompt for the NEXT input, as it goes in the command line: what is
    /// wanted and any options, "Specify next point or [Close/Undo]".
    fn prompt(&self, cx: &ToolContext<'_>) -> String;
    fn expects(&self, cx: &ToolContext<'_>) -> ToolInput;

    fn point(&mut self, cx: &ToolContext<'_>, at: Point2) -> ToolStep {
        let _ = at;
        ToolStep::rejected(format!(
            "a point is not expected here; the tool wants {}",
            to_string(self.expects(cx))
        ))
    }

    /// A point the view snapped to a named point of an entity - its end, its
    /// middle, its centre or a vertex (snap_anchor) - with the reference that
    /// names it, so annotation made from it can follow the entity, as one
    /// made from a typed #id.end point does (docs/annotation.md,
    /// "Associativity"). The default takes it as the plain point(at): only a
    /// tool whose annotation follows its points keeps the reference.
    fn anchored_point(&mut self, cx: &ToolContext<'_>, at: Point2, anchor: &AnchorRef) -> ToolStep {
        let _ = anchor;
        self.point(cx, at)
    }

    /// A point with a height, typed as x,y,z. The default drops the height
    /// and takes the point, so only a tool that draws in 3D (Polyline 3D,
    /// Point) overrides it; such a tool looks up the height of a clicked
    /// point itself (height_at_point), since a click arrives as point(). It is
    /// reached only at a step whose takes_heights() says so, so a tool that
    /// overrides this answers that too.
    fn point3d(&mut self, cx: &ToolContext<'_>, at: Point2, z: f64) -> ToolStep {
        let _ = z;
        self.point(cx, at)
    }

    /// A point given EXACTLY - typed as x,y, @dx,dy, @distance<angle or x,y,z
    /// (z its height, after takes_heights, dz_is_a_change and last_height have
    /// had their say) - rather than pointed at with the mouse. The default
    /// hands it on as a click goes, to point3d() with a height and point()
    /// without, so only a tool that treats a pointed click differently
    /// overrides it: Insert Vertex refuses a CLICK within the pick aperture of
    /// a vertex, where the vertex meant cannot be told from a new one, but a
    /// typed point says exactly where, whatever the zoom.
    fn exact_point(&mut self, cx: &ToolContext<'_>, at: Point2, z: Option<f64>) -> ToolStep {
        match z {
            Some(z) => self.point3d(cx, at, z),
            None => self.point(cx, at),
        }
    }

    /// Whether the step the tool is at takes a height with its point: the z
    /// of x,y,z, and the dz of @dx,dy,dz (read as dz_is_a_change says). A tool
    /// that takes none is handed the point alone, the z of either form
    /// dropped - as it always was; refusing @dx,dy,dz there turned "@0,5,0" at
    /// MOVE and LINE into an error. The default takes none.
    fn takes_heights(&self, cx: &ToolContext<'_>) -> bool {
        let _ = cx;
        false
    }

    /// Whether the dz of a typed @dx,dy,dz, at a step that takes heights, is
    /// a CHANGE of last_height() - Move Vertex's "raise it by dz", as the
    /// VERTEX MOVE verb reads it - rather than the height itself, as the
    /// PLINE3D verb reads it and so every other tool that takes a height (the
    /// default). A tool reads a line as its verb does, so the window and
    /// katana_cli make one drawing of it.
    fn dz_is_a_change(&self, cx: &ToolContext<'_>) -> bool {
        let _ = cx;
        false
    }

    fn entity(&mut self, cx: &ToolContext<'_>, id: EntityId, at: Point2) -> ToolStep {
        let _ = (id, at);
        ToolStep::rejected(format!(
            "an entity is not expected here; the tool wants {}",
            to_string(self.expects(cx))
        ))
    }

    /// Typed text that is not a point (route_typed_input decides): a
    /// distance, an angle in degrees, a count, a text string, or an option
    /// keyword from the prompt ("C" for Close).
    fn value(&mut self, cx: &ToolContext<'_>, text: &str) -> ToolStep {
        let _ = cx;
        ToolStep::rejected(format!("'{text}' is not an input this tool takes now"))
    }

    /// Enter, Space or a right-click: finish (Polyline), accept the default,
    /// or end a selection. The default finishes with nothing to do.
    fn enter(&mut self, cx: &ToolContext<'_>) -> ToolStep {
        let _ = cx;
        ToolStep::done(None, "", false)
    }

    /// Steps back one input (the U inside LINE). The default rejects.
    fn undo(&mut self, cx: &ToolContext<'_>) -> ToolStep {
        let _ = cx;
        ToolStep::rejected("nothing to undo in this tool")
    }

    /// Esc: the tool is ending, and this says what of its work stays. Done
    /// with a command keeps work the user has already placed step by step - a
    /// LINE chain's segments, the copies Copy has put down, the cuts a Trim
    /// has made - as AutoCAD keeps them; Done with no command drops the lot.
    /// It is not enter(): Enter at some steps applies a DEFAULT (Move's "the
    /// base point as the displacement", Copy's with nothing placed yet), which
    /// Esc must never do. The host ends the tool whatever this answers and
    /// ignores `restart`. The default keeps nothing; a tool whose Enter only
    /// commits work it has collected answers
    /// `tools::families::keep_work_on_escape(self, cx)`.
    fn cancel(&mut self, cx: &ToolContext<'_>) -> ToolStep {
        let _ = cx;
        ToolStep::done(None, "", false)
    }

    /// The rubber band for the cursor. The default draws nothing.
    fn preview(&self, cx: &ToolContext<'_>, cursor: Point2) -> ToolFeedback {
        let _ = (cx, cursor);
        ToolFeedback::default()
    }

    /// The last point accepted, which relative input (@dx,dy) and the
    /// Perpendicular and Tangent snaps measure from. None before the first.
    fn last_point(&self, cx: &ToolContext<'_>) -> Option<Point2> {
        let _ = cx;
        None
    }

    /// The height of last_point(), which the dz of a typed @dx,dy,dz changes
    /// (route_typed_input) at a step that takes heights, where
    /// dz_is_a_change(); None when it has none, and then @dx,dy,dz is refused
    /// there rather than read as a height of dz. The default has none.
    fn last_height(&self, cx: &ToolContext<'_>) -> Option<f64> {
        let _ = cx;
        None
    }

    /// Whether this tool takes the Delete key from the window's Erase while
    /// it runs. Erase holds Delete as a shortcut and erases the selection,
    /// and a vertex tool works on a SELECTED polyline - a chosen vertex's
    /// grips need it selected, and the tool selects what it has just edited
    /// (docs/drawing.md, R5) - so Delete inside the tool erased the whole
    /// string it was editing. The default leaves Delete to Erase.
    fn takes_delete(&self, cx: &ToolContext<'_>) -> bool {
        let _ = cx;
        false
    }

    /// Delete pressed while takes_delete(): what the tool makes of it. The
    /// default refuses it, and the refusal says why.
    fn delete_key(&mut self, cx: &ToolContext<'_>) -> ToolStep {
        let _ = cx;
        ToolStep::rejected("the Delete key does nothing in this tool")
    }

    /// Whether the view may snap the cursor to `snap` for the step the tool
    /// is at; one it may not is passed over for the next best
    /// (SnapRequest::accept), and with none the cursor stays where it is. A
    /// step that needs a point ON a polyline (Insert Vertex) takes only a
    /// snap that lands there. The default takes every snap.
    fn takes_snap(&self, cx: &ToolContext<'_>, snap: &SnapResult) -> bool {
        let _ = (cx, snap);
        true
    }

    /// The tool as Any, for what reaches a tool's own state from outside it
    /// (a test's sketch_move_for_tests): the C++'s dynamic_cast. A tool that
    /// is reached so answers `Some(self)`; the default is None.
    fn as_any_mut(&mut self) -> Option<&mut dyn Any> {
        None
    }
}

/// Parses a typed point. "x,y" is absolute; "@dx,dy" is relative to `last`;
/// "@distance<angle" is polar from `last`, the angle in degrees
/// counter-clockwise from east (the command line's convention). Fails with
/// InvalidState for relative input with no `last`, and ParseFailure
/// otherwise. Numbers go through core's text, so the decimal point is
/// always '.'.
pub fn parse_point_input(text: &str, last: Option<Point2>) -> Result<Point2> {
    // One grammar with the drafting aids' (drawing/drafting.rs), at the
    // command line's convention: degrees counter-clockwise from east, and a
    // DMS angle or a quadrant bearing after the < of polar input.
    Ok(parse_precise_point(text, last, &DraftingSettings::default())?.point)
}

// core's trimmed over text: it drops ASCII bytes only, so the rest is UTF-8.
fn trimmed(text: &str) -> &str {
    std::str::from_utf8(katana_core::text::trimmed(text.as_bytes())).unwrap_or_default()
}

// A typed point handed to `tool`, as the exact point it is (exact_point). A
// step that takes heights (takes_heights) gets the z of x,y,z as the height,
// and the dz of @dx,dy,dz as its verb reads it (dz_is_a_change): the height
// itself for Polyline 3D, as PLINE3D has it; a CHANGE of the last point's
// height for Move Vertex, as VERTEX MOVE has it - handing dz on as the height
// there turned "raise it by 1" into a height of 1 and wiped a surveyed z, and
// with no height there to change it is refused rather than read as absolute.
// Every other step drops the z of both forms, as it always did: refusing
// @dx,dy,dz there refused "@0,5,0" at MOVE and LINE, whose points have no
// height to change.
fn typed_point<T: InteractiveTool + ?Sized>(
    tool: &mut T,
    cx: &ToolContext<'_>,
    point: &PrecisePoint,
) -> ToolStep {
    let Some(z) = point.z.filter(|_| tool.takes_heights(cx)) else {
        return tool.exact_point(cx, point.point, None);
    };
    if !point.relative || !tool.dz_is_a_change(cx) {
        return tool.exact_point(cx, point.point, Some(z));
    }
    let Some(base) = tool.last_height(cx) else {
        return ToolStep::rejected(
            "the point @ is measured from has no height for a dz to change; give the height \
             absolutely, as x,y,z",
        );
    };
    tool.exact_point(cx, point.point, Some(base + z))
}

/// What the view and the command line do with typed text while a tool runs:
/// text that parses as a point is a point, anything else is a value. The one
/// place that decides, so a click and "10,20" typed are the same input.
pub fn route_typed_input<T: InteractiveTool + ?Sized>(
    tool: &mut T,
    cx: &ToolContext<'_>,
    text: &str,
) -> ToolStep {
    let input = trimmed(text);
    // A point only when it LOOKS like one - a comma, or the @ of relative
    // input. "12.5" is a distance or a radius, and must reach value() rather
    // than being refused as a malformed point.
    let looks_like_a_point =
        tool.expects(cx) != ToolInput::Value && (input.contains(',') || input.starts_with('@'));
    if looks_like_a_point {
        return match parse_precise_point(input, tool.last_point(cx), &DraftingSettings::default()) {
            Ok(point) => typed_point(tool, cx, &point),
            Err(error) => ToolStep::rejected(error.describe()),
        };
    }
    tool.value(cx, input)
}

/// A click the view snapped with `snap` (None when snapping found nothing or
/// is off), as the view hands it to `tool`: anchored_point when the snap
/// names a point of an entity of the context's document (snap_anchor), else
/// point. The one place that decides, so the plan view and the tests hand a
/// tool the same input. (The C++ takes the document as its own argument; here
/// it is the context's, and with none the click is a plain point.)
pub fn route_snapped_point<T: InteractiveTool + ?Sized>(
    tool: &mut T,
    cx: &ToolContext<'_>,
    at: Point2,
    snap: Option<&SnapResult>,
) -> ToolStep {
    if let (Some(snap), Some(document)) = (snap, cx.document)
        && let Some(anchor) = snap_anchor(document.model(), snap)
    {
        return tool.anchored_point(cx, at, &anchor);
    }
    tool.point(cx, at)
}

/// route_typed_input with the drafting aids (drawing/drafting.rs), as the
/// plan view and the command line route it (the C++ overload of
/// routeTypedInput):
///
/// ```text
///   <angle         locks the direction of the next points (< alone clears)
///   =distance      locks their distance (= alone clears)
///   x,y,z          a point with a height (InteractiveTool::point3d)
///   a number       the tool's value first (a radius, a count); if the tool
///                  refuses it at a point prompt, DIRECT DISTANCE ENTRY - the
///                  point that far from the last one towards `cursor`
/// ```
///
/// Points parse with the drafting settings' angle convention, so a bearing
/// or a DMS angle can follow the < of polar input. `drafting` is the host's
/// copy of the settings, written back by the host after the call (module
/// doc).
pub fn route_typed_input_with_drafting<T: InteractiveTool + ?Sized>(
    tool: &mut T,
    cx: &ToolContext<'_>,
    text: &str,
    drafting: &mut DraftingSettings,
    cursor: Option<Point2>,
) -> ToolStep {
    let input = trimmed(text);
    let at_point = tool.expects(cx) == ToolInput::Point;
    if at_point && input.starts_with('<') {
        let angle = trimmed(&input[1..]);
        if angle.is_empty() || equals_ignoring_case(angle.as_bytes(), b"off") {
            drafting.angle_lock = None;
            return ToolStep::next("angle lock off");
        }
        return match parse_direction(angle, drafting.angles) {
            Err(error) => ToolStep::rejected(error.message),
            Ok(direction) => {
                drafting.angle_lock = Some(direction);
                ToolStep::next(format!(
                    "angle locked at {} (bearing {})",
                    format_dms(direction * RAD_TO_DEG, 0),
                    format_bearing(direction, 0)
                ))
            }
        };
    }
    if at_point && input.starts_with('=') {
        let length = trimmed(&input[1..]);
        if length.is_empty() || equals_ignoring_case(length.as_bytes(), b"off") {
            drafting.length_lock = None;
            return ToolStep::next("length lock off");
        }
        let Some(value) = parse_finite_double(length).filter(|&value| value > 0.0) else {
            return ToolStep::rejected(format!(
                "'{length}' is not a length; type =distance, or = to clear the lock"
            ));
        };
        drafting.length_lock = Some(value);
        return ToolStep::next(format!("length locked at {}", format_exact_real(value)));
    }
    if tool.expects(cx) != ToolInput::Value && looks_like_point(input) {
        return match parse_precise_point(input, tool.last_point(cx), drafting) {
            Ok(point) => typed_point(tool, cx, &point),
            Err(error) => ToolStep::rejected(error.describe()),
        };
    }
    let step = tool.value(cx, input);
    if step.outcome == ToolStepOutcome::Rejected
        && at_point
        && let Some(last) = tool.last_point(cx)
        && let Some(distance) = parse_finite_double(input)
    {
        return tool.point(
            cx,
            direct_distance(last, cursor.unwrap_or(last), distance, drafting),
        );
    }
    step
}

// ---- the catalogue -----------------------------------------------------------------

/// What makes a tool: handed the context of its make and what it starts with
/// (module doc). Shared, as the C++ std::function in ToolInfo is copied.
pub type ToolFactory =
    Arc<dyn Fn(&ToolContext<'_>, &ToolStart) -> Box<dyn InteractiveTool> + Send + Sync>;

#[derive(Clone, Default)]
pub struct ToolInfo {
    /// Stable identity, "draw.line": menus, toolbars, settings, shortcuts and
    /// tests name a tool by it, so it never changes once shipped.
    pub id: String,
    /// As the menu shows it: "Line".
    pub name: String,
    /// What the prompt calls the tool, when not its name: a family's variant
    /// named for what it does ("Insert Vertex", not "Vertices, Insert
    /// Vertex"). Empty for the name. prompt_title() chooses.
    pub title: String,
    /// The menu it goes in: "Draw", "Modify", "Annotate", "Inquiry".
    pub category: String,
    /// The section within that menu: "Lines", "Curves", "Transform", ...
    pub group: String,
    /// Position within the group, ascending.
    pub order: i32,
    /// Command-line verbs that start it, upper case: {"LINE", "L"}. Unique
    /// across the catalogue - one word, one tool.
    pub aliases: Vec<String>,
    /// A key sequence ("Ctrl+Shift+L"), or empty. Single letters are not
    /// allowed: a letter typed into a view goes to the command line.
    pub shortcut: String,
    /// One sentence: what it does and how.
    pub tip: String,
    /// None is refused by the catalogue (the C++'s empty std::function).
    pub make: Option<ToolFactory>,
}

impl fmt::Debug for ToolInfo {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ToolInfo")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("title", &self.title)
            .field("category", &self.category)
            .field("group", &self.group)
            .field("order", &self.order)
            .field("aliases", &self.aliases)
            .field("shortcut", &self.shortcut)
            .field("tip", &self.tip)
            .field("make", &self.make.is_some())
            .finish()
    }
}

impl ToolInfo {
    /// What the view's prompt band and the command line's placeholder put
    /// before the prompt: the title, or the name when there is none.
    pub fn prompt_title(&self) -> &str {
        if self.title.is_empty() {
            &self.name
        } else {
            &self.title
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct ToolCatalog {
    tools: Vec<ToolInfo>,
}

impl ToolCatalog {
    pub fn new() -> Self {
        Self::default()
    }

    /// InvalidArgument for an empty id, name, category or group, no factory,
    /// a lower-case or empty alias, or a single-letter shortcut;
    /// AlreadyExists for an id or an alias already taken.
    pub fn add(&mut self, info: ToolInfo) -> Status {
        if info.id.is_empty()
            || info.name.is_empty()
            || info.category.is_empty()
            || info.group.is_empty()
        {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "a tool needs an id, a name, a category and a group",
                info.id.as_str(),
            ));
        }
        if info.make.is_none() {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "a tool needs a factory",
                info.id.as_str(),
            ));
        }
        // One visible character is a letter typed into the view, which goes
        // to the command line - a shortcut there would steal it. (The C++
        // counts bytes.)
        if info.shortcut.len() == 1 {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "a single-key shortcut would steal a letter typed into the view",
                format!("{} {}", info.id, info.shortcut),
            ));
        }
        for alias in &info.aliases {
            let has_lower = alias.bytes().any(|byte| byte.is_ascii_lowercase());
            if alias.is_empty() || has_lower {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "an alias is a non-empty upper-case verb",
                    format!("{} '{alias}'", info.id),
                ));
            }
            if let Some(other) = self.find_by_alias(alias) {
                return Err(make_error(
                    ErrorCode::AlreadyExists,
                    "the alias is taken by another tool",
                    format!("{} {alias} ({})", info.id, other.id),
                ));
            }
        }
        if self.find(&info.id).is_some() {
            return Err(make_error(
                ErrorCode::AlreadyExists,
                "a tool with this id already exists",
                info.id.as_str(),
            ));
        }
        self.tools.push(info);
        Ok(())
    }

    pub fn find(&self, id: &str) -> Option<&ToolInfo> {
        self.tools.iter().find(|tool| tool.id == id)
    }

    /// By command-line verb, case-insensitively.
    pub fn find_by_alias(&self, verb: &str) -> Option<&ToolInfo> {
        self.tools.iter().find(|tool| {
            tool.aliases
                .iter()
                .any(|alias| equals_ignoring_case(alias.as_bytes(), verb.as_bytes()))
        })
    }

    /// Ordered by category, then group, then order, then id: the menu order,
    /// the same on every run.
    pub fn all(&self) -> Vec<&ToolInfo> {
        let mut out: Vec<&ToolInfo> = self.tools.iter().collect();
        out.sort_by(|a, b| {
            (&a.category, &a.group, a.order, &a.id).cmp(&(&b.category, &b.group, b.order, &b.id))
        });
        out
    }

    pub fn size(&self) -> usize {
        self.tools.len()
    }
}

struct BuiltCatalog {
    catalog: ToolCatalog,
    problems: Vec<String>,
}

fn built_catalog() -> &'static BuiltCatalog {
    // Built on first use. Each family adds its own tools; a refusal is kept,
    // not thrown, and a test asserts there are none.
    static BUILT: OnceLock<BuiltCatalog> = OnceLock::new();
    BUILT.get_or_init(|| {
        let mut catalog = ToolCatalog::new();
        let mut problems = Vec::new();
        let mut report = |status: Status| {
            if let Err(error) = status {
                problems.push(error.describe());
            }
        };
        for family in FAMILIES {
            family(&mut catalog, &mut report);
        }
        BuiltCatalog { catalog, problems }
    })
}

/// The program's catalogue, built on first use from every tool family's list
/// (tools/families.rs, FAMILIES). A family whose tool fails to register is a
/// programming error; building it throws nothing but keeps the refusals in
/// tool_catalog_problems(), which a test asserts is empty.
pub fn tool_catalog() -> &'static ToolCatalog {
    &built_catalog().catalog
}

pub fn tool_catalog_problems() -> &'static [String] {
    &built_catalog().problems
}
