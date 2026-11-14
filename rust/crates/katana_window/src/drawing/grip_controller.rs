//! The plan view's grips (docs/drawing.md, "Grips"): with entities selected and no tool running, a
//! square on every vertex, segment middle, centre, quadrant and insertion point, which the user
//! hovers, clicks and drags. The port of drawing/grip_controller.
//!
//! What a grip IS and what dragging it DOES are katana_cad's (drawing/grips.rs); this is only the
//! state of the gesture and the drawing of the squares, so the view forwards its mouse and keys
//! here before its own selection handling:
//!
//! ```text
//!   press on a grip        grab it; plain: it and only it hot, unless it was already hot (then
//!                          every hot grip moves); Shift: toggles it hot without grabbing
//!   drag, release          the hot grips move with the cursor - snapped, tracked, ortho or polar
//!                          as the view constrains it - and the release is ONE undoable GRIP_EDIT
//!   click, move, click     the same without holding the button (the grip is picked up by a
//!                          click and put down by the next)
//!   typed while grabbed    x,y / @dx,dy / @distance<angle or bearing / a plain distance along
//!                          the cursor, then Enter
//!   Delete                 removes the hot vertex grips' vertices (one step)
//!   Esc                    drops a grabbed grip, then the hot set
//!   hover                  the view's band says what the grip is and what can be done with it
//!                          (hover_hint)
//!   Ctrl held on a middle  the segment middle is drawn as the vertex a Ctrl-drag there would add
//!   right-click            the grip's own items in the shortcut menu (plan_context_menu), which
//!                          hand it to a tool through set_hot
//! ```
//!
//! Everything is in model coordinates except the aperture tests, which the view converts; nothing
//! here knows the view's transform.
//!
//! The Rust shape. The C++ controller keeps the document and a pointer to the view's hidden
//! layers; here each call that reads or edits the drawing is lent the document, and `refresh` is
//! lent the layers. A refusal goes into `errors`, which the view hands on (the C++ `onError`); the
//! C++ `onMessage` was never raised and has no twin.

use katana_cad::drawing::drafting::{direct_distance, looks_like_point, parse_precise_point};
use katana_cad::drawing::grips::{
    DEFAULT_GRIP_LIMIT, Grip, GripDrag, GripKind, delete_hot_vertices, grip_at, grip_drag_command,
    grip_feedback, grips_of_selection,
};
use katana_cad::drawing::vertex_editing::{height_text, read_polyline};
use katana_cad::tool_feedback::{FeedbackMark, FeedbackRole, ToolFeedback};
use katana_cad::{Document, LayerOverrides, NO_LAYER_OVERRIDES};
use katana_core::text::parse_finite_double;
use katana_entity::entity::to_string;
use katana_entity::{EntityId, Geometry, PointGeometry};
use katana_geometry::Point2;
use katana_math::numerics::tolerance;

use super::feedback_painter::{FeedbackFrame, darker, overlay, paint_feedback};
use crate::canvas::{Brush, Canvas, FillRule, Pen, PenCap, PenJoin, PenStyle, PointF, RectF};

// AutoCAD's grip colours, which a drafter's eye already reads - blue cold, green under the cursor,
// red hot - are the overlay's (feedback_painter), as is the rubber band's cyan, a tool preview's.
const GRIP_HALF_PIXELS: f64 = 4.0;
const DRAG_PIXELS: f64 = 4.0;

/// The places of `entity`'s vertex grips, by index; empty for an entity with none (a circle, a
/// text).
fn vertex_places(grips: &[Grip], entity: EntityId) -> Vec<Option<Point2>> {
    let mut places: Vec<Option<Point2>> = Vec::new();
    for grip in grips {
        if grip.entity != entity || grip.kind != GripKind::Vertex {
            continue;
        }
        if grip.index >= places.len() {
            places.resize(grip.index + 1, None);
        }
        places[grip.index] = Some(grip.position);
    }
    places
}

/// Whether a polyline grip's own vertices are all that moved from `before` to `grips`: every other
/// vertex of its entity is where it was, index for index, and there are as many. A vertex grip's
/// own vertex is itself; a segment middle's are the segment's two ends.
fn only_its_own_vertices_moved(old: &Grip, grips: &[Grip], before: &[Grip]) -> bool {
    let was = vertex_places(before, old.entity);
    let now = vertex_places(grips, old.entity);
    if was.len() != now.len() || now.is_empty() {
        return false;
    }
    let n = now.len();
    for i in 0..n {
        let own = if old.kind == GripKind::Vertex {
            i == old.index
        } else {
            i == old.index || i == (old.index + 1) % n
        };
        if own {
            continue;
        }
        match (was[i], now[i]) {
            (Some(was), Some(now)) if was.distance_to(now) <= tolerance::GEOMETRIC => {}
            _ => return false,
        }
    }
    true
}

/// The grip that is `old` after the drawing changed from `before` to `grips`: the same point of
/// the same entity, by its position - an index names another vertex once a vertex is inserted or
/// deleted before it, and the next Delete would take that one. The same index first, where a twin
/// vertex lies on the same point.
///
/// Where no grip is at its point, its own vertex may have been moved (VERTEX MOVE, a Vertices
/// panel cell, another view), and matched by position alone such a move silently unchose it. A
/// polyline's vertex or segment middle then keeps its index only when every OTHER vertex of the
/// polyline is where it was: that is a move of its own. Two edits between refreshes - a SCRIPT,
/// lines pasted, an agent - can delete the chosen vertex and insert another, leaving as many
/// vertices as before; kept by its index then, the grip named a vertex nobody chose, and Enter or
/// Delete removed it. It goes instead. A grip of any other kind (a centre, a quadrant, a line's
/// end) keeps its index while its entity has as many of that kind: those are never renumbered.
fn follow(old: &Grip, grips: &[Grip], before: &[Grip]) -> Option<Grip> {
    let same = |grip: &Grip| grip.entity == old.entity && grip.kind == old.kind;
    let at =
        |grip: &Grip| same(grip) && grip.position.distance_to(old.position) <= tolerance::GEOMETRIC;
    if let Some(found) = grips
        .iter()
        .find(|grip| grip.index == old.index && at(grip))
    {
        return Some(*found);
    }
    if let Some(found) = grips.iter().find(|grip| at(grip)) {
        return Some(*found);
    }
    let renumbered = old.kind == GripKind::Vertex || old.kind == GripKind::SegmentMid;
    let kept = if renumbered {
        only_its_own_vertices_moved(old, grips, before)
    } else {
        grips.iter().filter(|grip| same(grip)).count()
            == before.iter().filter(|grip| same(grip)).count()
    };
    if kept {
        return grips
            .iter()
            .find(|grip| grip.index == old.index && same(grip))
            .copied();
    }
    None
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum State {
    #[default]
    Idle,
    Pressed,
    Dragging,
    Attached,
}

/// See the module comment.
#[derive(Debug)]
pub struct GripController {
    /// What the view's hidden layers held when the grips were last built.
    built_for: LayerOverrides,
    generation: u64,
    grips: Vec<Grip>,
    hot: Vec<Grip>,
    hovered: Option<Grip>,
    grabbed: Option<Grip>,
    state: State,
    insert: bool,
    ctrl: bool,
    target: Point2,
    raw_cursor: Point2,
    typed: String,
    /// What a put-down, a Delete or a typed point was refused with, in order, until the view
    /// takes them (the C++ `onError`).
    pub errors: Vec<String>,
}

impl Default for GripController {
    fn default() -> Self {
        Self::new()
    }
}

impl GripController {
    pub fn new() -> Self {
        Self {
            built_for: LayerOverrides::default(),
            // No count of notifications is this one: the first refresh always builds.
            generation: u64::MAX,
            grips: Vec::new(),
            hot: Vec::new(),
            hovered: None,
            grabbed: None,
            state: State::Idle,
            insert: false,
            ctrl: false,
            target: Point2::default(),
            raw_cursor: Point2::default(),
            typed: String::new(),
            errors: Vec::new(),
        }
    }

    /// Rebuilds the grips from the document's selection when `generation` (the view's count of
    /// document notifications) has moved on, or `view` - the layers the view hides of its own,
    /// which change with no notification - is not what they were built from. A selected entity on
    /// a layer the view hides offers no grip here (cad's grips_of_selection): it is a ghost in
    /// this view at most. None is the document's rule alone. A hot or a grabbed grip follows its
    /// point to the index it has now - after an insert before it, vertex 2 is vertex 3 - keeps its
    /// index when its own vertex alone was moved, and goes when its vertex does, or when edits
    /// since have left it no telling which vertex it was.
    pub fn refresh(&mut self, document: &Document, view: Option<&LayerOverrides>, generation: u64) {
        let view = view.unwrap_or(&NO_LAYER_OVERRIDES);
        if generation == self.generation && *view == self.built_for {
            return;
        }
        self.generation = generation;
        self.built_for = view.clone();
        let before = std::mem::replace(
            &mut self.grips,
            grips_of_selection(
                document,
                &document.selection().ids(),
                view,
                DEFAULT_GRIP_LIMIT,
            ),
        );
        // A hot grip survives a refresh only while its vertex does (an undo, a selection change
        // or another view's edit may have taken it), and it follows its vertex to the index it
        // has now (follow).
        let kept: Vec<Grip> = self
            .hot
            .iter()
            .filter_map(|hot| follow(hot, &self.grips, &before))
            .collect();
        self.hot = kept;
        if let Some(grabbed) = self.grabbed {
            match follow(&grabbed, &self.grips, &before) {
                Some(now) => self.grabbed = Some(now),
                None => {
                    self.grabbed = None;
                    self.state = State::Idle;
                    self.typed.clear();
                }
            }
        }
    }

    pub fn grips(&self) -> &[Grip] {
        &self.grips
    }

    pub fn hot(&self) -> &[Grip] {
        &self.hot
    }

    pub fn hovered(&self) -> Option<Grip> {
        self.hovered
    }

    /// Whether a vertex grip is hot: Delete is then the view's, for the vertices, and not the
    /// window's Erase for the whole polyline.
    pub fn has_hot_vertex(&self) -> bool {
        self.hot.iter().any(|grip| grip.kind == GripKind::Vertex)
    }

    /// What the view's band says while a grip is under the cursor and none is grabbed: what it is,
    /// and what can be done with it - "Vertex 2 of polyline 12, z 101.500: drag to move · click to
    /// pick up · ...". Empty with no grip hovered or one grabbed (`prompt` speaks then).
    pub fn hover_hint(&self, document: &Document) -> String {
        let Some(grip) = self.hovered else {
            return String::new();
        };
        if self.grabbed.is_some() {
            return String::new();
        }
        let Some(entity) = document.model().entities.find(grip.entity) else {
            return String::new();
        };
        // A polyline's vertex and segment middle, the grips vertex editing is about, say what
        // else they offer: the shortcut menu's items, and the gestures nothing else on screen
        // mentions (Shift to choose, Ctrl to add).
        if let Some(polyline) = read_polyline(entity) {
            if grip.kind == GripKind::Vertex && grip.index < polyline.vertices.len() {
                // Its height written as the vertex tools' labels write it (cad's height_text):
                // "z -0.000" here beside "z 0.000" there was one vertex read two ways.
                let height = polyline.vertices[grip.index].height;
                let said = if height.is_some() {
                    format!(", {}", height_text(height))
                } else {
                    String::new()
                };
                return format!(
                    "Vertex {} of polyline {}{}: drag to move · click to pick up · Shift+click to \
                     choose · Delete removes the chosen · right-click for vertex tools",
                    grip.index, grip.entity, said
                );
            }
            if grip.kind == GripKind::SegmentMid {
                // A straight segment's middle moves the segment bodily; an arc's keeps both ends
                // and bends the arc through the cursor (docs/drawing.md, "What a drag means per
                // handle").
                let arc = grip.index < polyline.segment_count() && polyline.is_arc(grip.index);
                return format!(
                    "Segment {} of polyline {}: drag to {} · Ctrl+drag to add a vertex · \
                     right-click for segment tools",
                    grip.index,
                    grip.entity,
                    if arc { "bend the arc" } else { "stretch" }
                );
            }
        }
        let name = to_string(grip.kind);
        let mut kind = String::with_capacity(name.len());
        let mut letters = name.chars();
        if let Some(first) = letters.next() {
            kind.extend(first.to_uppercase());
            kind.push_str(letters.as_str());
        }
        format!(
            "{} of {} {}: drag to move it · click to pick it up · Shift+click to choose",
            kind,
            to_string(entity.r#type()),
            grip.entity
        )
    }

    /// Ctrl held, as the view hears it from its keys and mouse moves: a hovered segment middle is
    /// drawn as the vertex a Ctrl-drag would add.
    pub fn set_ctrl(&mut self, held: bool) {
        self.ctrl = held;
    }

    pub fn ctrl(&self) -> bool {
        self.ctrl
    }

    /// `grips` hot and nothing grabbed: the shortcut menu opened on a grip hands it to the tool
    /// it starts, as a click on the grip before the tool would (ToolStart::handles).
    pub fn set_hot(&mut self, grips: Vec<Grip>) {
        self.grabbed = None;
        self.state = State::Idle;
        self.typed.clear();
        self.insert = false;
        self.hot = grips;
    }

    /// True while a grip is held (dragged) or picked up: the view then snaps and constrains the
    /// cursor from `base` and sends every move here.
    pub fn active(&self) -> bool {
        self.state == State::Dragging || self.state == State::Attached
    }

    pub fn pressed(&self) -> bool {
        self.state == State::Pressed
    }

    /// Where the grabbed grip was: the base of the drag's rubber band, of relative typed input and
    /// of the ortho and polar constraints.
    pub fn base(&self) -> Option<Point2> {
        self.grabbed.map(|grabbed| grabbed.position)
    }

    pub fn typed(&self) -> &str {
        &self.typed
    }

    pub fn prompt(&self) -> String {
        let Some(grabbed) = self.grabbed else {
            return String::new();
        };
        // A Ctrl-press on a segment middle adds a vertex where it is put down; the band said what
        // a stretch says, word for word.
        if self.insert && grabbed.kind == GripKind::SegmentMid {
            return format!(
                "Grip segment middle, adding a vertex: specify where it goes or type x,y, @dx,dy \
                 or @distance<angle  {}_",
                self.typed
            );
        }
        format!(
            "Grip {}: specify the point or type x,y, @dx,dy or @distance<angle  {}_",
            to_string(grabbed.kind),
            self.typed
        )
    }

    fn is_hot(&self, grip: &Grip) -> bool {
        self.hot.iter().any(|hot| hot.same_handle(grip))
    }

    // ---- what the view forwards; each returns true when it took the input --------------------

    /// A left press at `at` (model), `aperture` the grip's reach in model units; Ctrl on a segment
    /// middle inserts a vertex instead of stretching.
    pub fn press(
        &mut self,
        document: &mut Document,
        at: Point2,
        aperture: f64,
        shift: bool,
        ctrl: bool,
    ) -> bool {
        if self.state == State::Attached {
            // The grip was picked up by a click; this click puts it down, where the cursor is
            // (snapped and constrained by the view).
            self.commit_at(document, self.target);
            return true;
        }
        let Some(found) = grip_at(&self.grips, at, aperture) else {
            return false;
        };
        if shift {
            if self.is_hot(&found) {
                self.hot.retain(|hot| !hot.same_handle(&found));
            } else {
                self.hot.push(found);
            }
            return true;
        }
        if !self.is_hot(&found) {
            self.hot = vec![found];
        }
        self.grabbed = Some(found);
        self.insert = ctrl;
        self.target = found.position;
        self.raw_cursor = at;
        self.state = State::Pressed;
        true
    }

    /// The cursor, already snapped and constrained by the view. `moved_pixels` is how far the
    /// mouse has gone since the press, to tell a drag from a click.
    pub fn move_to(&mut self, target: Point2, raw: Point2, aperture: f64, moved_pixels: f64) {
        self.raw_cursor = raw;
        if self.state == State::Pressed && moved_pixels > DRAG_PIXELS {
            self.state = State::Dragging;
        }
        if self.active() {
            self.target = target;
            self.hovered = None;
            return;
        }
        self.hovered = grip_at(&self.grips, raw, aperture);
    }

    pub fn release(&mut self, document: &mut Document) -> bool {
        if self.state == State::Dragging {
            self.commit_at(document, self.target);
            return true;
        }
        if self.state == State::Pressed {
            // A click without a drag picks the grip up.
            self.state = State::Attached;
            return true;
        }
        false
    }

    pub fn escape(&mut self) -> bool {
        if self.grabbed.is_some() {
            self.grabbed = None;
            self.state = State::Idle;
            self.typed.clear();
            self.insert = false;
            return true;
        }
        if !self.hot.is_empty() {
            self.hot.clear();
            return true;
        }
        false
    }

    pub fn delete_hot(&mut self, document: &mut Document) -> bool {
        let any_vertex = self
            .hot
            .iter()
            .any(|grip| grip.kind == GripKind::Vertex || grip.kind == GripKind::LeaderVertex);
        if !any_vertex {
            return false;
        }
        if let Err(error) = document.execute(delete_hot_vertices(self.hot.clone())) {
            self.errors.push(error.describe());
        }
        self.hot.clear();
        self.grabbed = None;
        self.state = State::Idle;
        true
    }

    pub fn enter(&mut self, document: &mut Document) -> bool {
        let Some(grabbed) = self.grabbed else {
            return false;
        };
        if self.typed.trim().is_empty() {
            self.commit_at(document, self.target);
            return true;
        }
        let text = String::from(self.typed.trim());
        self.typed.clear();
        let drafting = *document.drafting();
        if looks_like_point(&text) {
            match parse_precise_point(&text, Some(grabbed.position), &drafting) {
                Err(error) => self.errors.push(error.describe()),
                Ok(point) => self.commit_at(document, point.point),
            }
            return true;
        }
        if let Some(distance) = parse_finite_double(&text) {
            // Direct distance entry: that far from where the grip was, towards the cursor.
            let to = direct_distance(grabbed.position, self.raw_cursor, distance, &drafting);
            self.commit_at(document, to);
            return true;
        }
        self.errors.push(format!(
            "'{text}' is not a point or a distance; type x,y, @dx,dy, @distance<angle or a \
             distance"
        ));
        true
    }

    pub fn type_text(&mut self, text: &str) -> bool {
        if self.grabbed.is_none() {
            return false;
        }
        self.typed.push_str(text);
        true
    }

    pub fn backspace(&mut self) -> bool {
        if self.grabbed.is_none() {
            return false;
        }
        self.typed.pop();
        true
    }

    /// Drops everything: a new drawing, a tool starting.
    pub fn reset(&mut self) {
        self.hot.clear();
        self.grabbed = None;
        self.hovered = None;
        self.state = State::Idle;
        self.typed.clear();
        self.insert = false;
        self.generation = u64::MAX;
    }

    fn drag_to(&self, grabbed: Grip, target: Point2) -> GripDrag {
        GripDrag {
            grabbed,
            target,
            insert_vertex: self.insert,
            also_hot: self
                .hot
                .iter()
                .filter(|hot| !hot.same_handle(&grabbed))
                .copied()
                .collect(),
        }
    }

    fn commit_at(&mut self, document: &mut Document, target: Point2) {
        let Some(grabbed) = self.grabbed else {
            return;
        };
        let drag = self.drag_to(grabbed, target);
        self.grabbed = None;
        self.state = State::Idle;
        self.typed.clear();
        self.insert = false;
        self.hot.clear();
        if drag.target.distance_to(drag.grabbed.position) <= tolerance::GEOMETRIC
            && !drag.insert_vertex
        {
            // Put down where it was: nothing to do, and no empty undo step.
            return;
        }
        if let Err(error) = document.execute(grip_drag_command(drag)) {
            self.errors.push(error.describe());
        }
    }

    /// The grips, the drag's preview and its rubber band, onto `canvas` through `to_screen`; only
    /// grips inside `visible` (screen pixels). `draw_shape` draws a geometry in the canvas's
    /// current pen, as the plan painter draws one.
    pub fn paint(
        &self,
        canvas: &mut dyn Canvas,
        document: &Document,
        to_screen: &dyn Fn(Point2) -> PointF,
        draw_shape: &mut dyn FnMut(&mut dyn Canvas, &Geometry),
        visible: RectF,
    ) {
        let mut frame = FeedbackFrame {
            to_screen: Box::new(to_screen),
            draw_shape: Box::new(|canvas: &mut dyn Canvas, geometry: &Geometry| {
                draw_shape(canvas, geometry);
            }),
            visible,
            cursor: PointF::default(),
            tracking_label: false,
            band_height: 0.0,
            beneath_glyphs: None,
        };
        if let Some(grabbed) = self.grabbed
            && self.active()
        {
            // The drag as a tool's preview is drawn (cad's grip_feedback): the edited geometry
            // dashed, the grip's old place, and the vertex a Ctrl-drag adds.
            let drag = self.drag_to(grabbed, self.target);
            frame.cursor = to_screen(self.target);
            let _ = paint_feedback(canvas, &grip_feedback(document, &drag), &[], &mut frame);
            canvas.set_pen(Pen::with(
                overlay::preview(),
                1.0,
                PenStyle::Dot,
                PenCap::Square,
                PenJoin::Bevel,
            ));
            canvas.set_brush(Brush::none());
            canvas.draw_line(to_screen(grabbed.position), to_screen(self.target));
        }
        canvas.set_brush(Brush::none());
        for grip in &self.grips {
            let p = to_screen(grip.position);
            if !visible.contains(p) {
                continue;
            }
            let mut colour = overlay::grip_cold();
            if self.is_hot(grip) {
                colour = overlay::grip_hot();
            } else if self
                .hovered
                .is_some_and(|hovered| hovered.same_handle(grip))
            {
                colour = overlay::grip_hover();
            }
            canvas.set_pen(Pen::new(darker(colour, 150)));
            canvas.set_brush(Brush::solid(colour));
            let r = if grip.kind == GripKind::SegmentMid {
                GRIP_HALF_PIXELS - 1.0
            } else {
                GRIP_HALF_PIXELS
            };
            if grip.kind == GripKind::SegmentMid {
                // A segment's middle is a smaller diamond, so it does not read as a vertex at a
                // glance.
                canvas.draw_polygon(
                    &[
                        PointF::new(p.x, p.y - r - 1.0),
                        PointF::new(p.x + r + 1.0, p.y),
                        PointF::new(p.x, p.y + r + 1.0),
                        PointF::new(p.x - r - 1.0, p.y),
                    ],
                    FillRule::default(),
                );
            } else if grip.kind == GripKind::Centre {
                canvas.draw_ellipse(RectF::new(p.x - r, p.y - r, 2.0 * r, 2.0 * r));
            } else {
                canvas.draw_rect(RectF::new(p.x - r, p.y - r, 2.0 * r, 2.0 * r));
            }
        }
        canvas.set_brush(Brush::none());
        // Ctrl held over a segment middle: it is drawn as the vertex a drag there adds, the
        // gesture's only sign on screen.
        if self.ctrl
            && !self.active()
            && let Some(hovered) = self.hovered
            && hovered.kind == GripKind::SegmentMid
        {
            let added = ToolFeedback {
                marks: vec![FeedbackMark {
                    role: FeedbackRole::Added,
                    geometry: Geometry::Point(PointGeometry {
                        position: hovered.position,
                    }),
                    ..FeedbackMark::default()
                }],
                ..ToolFeedback::default()
            };
            frame.cursor = to_screen(hovered.position);
            let _ = paint_feedback(canvas, &added, &[], &mut frame);
        }
    }
}
