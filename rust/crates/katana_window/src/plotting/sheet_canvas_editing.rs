//! The sheet canvas's selection of several viewports, its view of the paper and the cursor
//! (docs/plotting.md, "Editing on the canvas"): SheetCanvas members kept out of sheet_editor,
//! which holds the canvas's painting and its mouse, so that file changes only where a drag, a band
//! or a key is handled. The port of plotting/sheet_canvas_editing.
//!
//! ```text
//! the selection   any number of the sheet's viewports, one of them the primary (selected());
//!                 pruned after every change, so an undo cannot leave a viewport selected that is
//!                 gone
//! the view        zoom to the selection, the paper grid and the rulers
//! the cursor      the paper and world position under it (sheet_readout), a plan's "auto"
//!                 decided once per change
//! a drag          where each viewport is drawn while it moves, its own paint going with it, and
//!                 Escape abandoning it
//! ```

use std::collections::BTreeMap;

use katana_cad::document::Document;
use katana_cad::plotting::sheet_set::Viewport;
use katana_cad::plotting::viewport_edits::{cycle_viewport, viewport_bounds};
use katana_geometry::{Box2, Point2};

use crate::canvas::{Brush, Canvas, Color, IntRect, Pen, PenCap, PenJoin, PenStyle, PointF, RectF};
use crate::plotting::sheet_readout::{SheetCursorReadout, sheet_cursor_readout};
use crate::plotting::sheet_rulers::SHEET_RULER_PIXELS;
use crate::sheet_editor::{CanvasCursor, Drag, SheetCanvas, SheetKey};
use crate::sheet_painter::{ResolvedViewport, resolve_plan_viewport_on_sheet};

impl SheetCanvas {
    // ---- the selection ---------------------------------------------------------------------

    /// What the C++ did on a selection change: a paint (the next frame's), and the properties and
    /// the edit actions built again from it, which they do once they see the serial move.
    pub(crate) fn selection_changed(&mut self) {
        self.selection_serial += 1;
    }

    /// Every selected viewport's id, in the sheet's order (back to front).
    pub fn selected_ids(&self) -> Vec<String> {
        self.current_sheet()
            .map(|sheet| {
                sheet
                    .viewports
                    .iter()
                    .filter(|viewport| self.is_selected(&viewport.id))
                    .map(|viewport| viewport.id.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn is_selected(&self, viewport_id: &str) -> bool {
        !viewport_id.is_empty() && self.selection.iter().any(|id| id == viewport_id)
    }

    /// Selects `ids` (those not on the sheet are dropped) with `primary` as the primary, or the
    /// last of `ids` when `primary` is empty or not among them.
    pub fn set_selection(&mut self, ids: &[String], primary: &str) {
        let mut kept: Vec<String> = Vec::new();
        for id in ids {
            if self.viewport(id).is_some() && !kept.contains(id) {
                kept.push(id.clone());
            }
        }
        let primary = if kept.iter().any(|id| id == primary) {
            String::from(primary)
        } else {
            kept.last().cloned().unwrap_or_default()
        };
        if kept == self.selection && primary == self.selected {
            return;
        }
        self.selection = kept;
        self.selected = primary;
        self.selection_changed();
    }

    /// Ctrl-click: `viewport_id` into the selection as its primary, or out of it.
    pub fn toggle_selected(&mut self, viewport_id: &str) {
        if self.viewport(viewport_id).is_none() {
            return;
        }
        let mut ids = self.selection.clone();
        if self.is_selected(viewport_id) {
            ids.retain(|id| id != viewport_id);
            let primary = if self.selected == viewport_id {
                String::new()
            } else {
                self.selected.clone()
            };
            self.set_selection(&ids, &primary);
        } else {
            ids.push(String::from(viewport_id));
            self.set_selection(&ids, viewport_id);
        }
    }

    pub fn select_all(&mut self) {
        let ids: Vec<String> = self
            .current_sheet()
            .map(|sheet| {
                sheet
                    .viewports
                    .iter()
                    .filter(|viewport| !viewport.rect.empty())
                    .map(|viewport| viewport.id.clone())
                    .collect()
            })
            .unwrap_or_default();
        let primary = self.selected.clone();
        self.set_selection(&ids, &primary);
    }

    /// Tab: the next (previous) viewport in the sheet's order, alone.
    pub fn cycle_selection(&mut self, forward: bool) {
        if let Some(sheet) = self.current_sheet() {
            let next = cycle_viewport(sheet, &self.selected, forward);
            self.select(&next);
        }
    }

    /// Drops from the selection what is no longer on the sheet: after an undo, a delete, a paste
    /// that was undone.
    pub(crate) fn prune_selection(&mut self) {
        let gone = |canvas: &Self, id: &str| canvas.viewport(id).is_none();
        if !self.selection.iter().any(|id| gone(self, id))
            && (self.selected.is_empty() || !gone(self, &self.selected))
        {
            return;
        }
        let kept: Vec<String> = self
            .selection
            .iter()
            .filter(|id| !gone(self, id))
            .cloned()
            .collect();
        self.selection = kept;
        if self.selected.is_empty() || gone(self, &self.selected) {
            self.selected = self.selection.last().cloned().unwrap_or_default();
        }
        self.selection_changed();
    }

    // ---- the view --------------------------------------------------------------------------

    /// Where the canvas's paper view starts: below and right of the rulers.
    pub(crate) fn ruler_pixels(&self) -> f64 {
        if self.rulers {
            f64::from(SHEET_RULER_PIXELS)
        } else {
            0.0
        }
    }

    pub fn zoom_to_selection(&mut self) {
        let Some(sheet) = self.current_sheet() else {
            return;
        };
        let bounds = viewport_bounds(sheet, &self.selected_ids());
        self.zoom_to(&bounds);
    }

    /// The paper grid (cad's PAPER_GRID_MM) a drag snaps to, drawn faintly while on.
    pub fn set_snap_to_grid(&mut self, on: bool) {
        self.snap_grid = on;
    }

    /// The rulers along the top and left, in paper millimetres. The paper stays where it is; the
    /// rulers cover or uncover the desk.
    pub fn set_rulers_shown(&mut self, on: bool) {
        self.rulers = on;
    }

    // ---- the cursor ------------------------------------------------------------------------

    /// The drawn scale and centre of a plan, "auto" decided; kept until the next change, so the
    /// cursor readout costs nothing per move.
    pub(crate) fn resolved_plan(&self, document: &Document, v: &Viewport) -> ResolvedViewport {
        if !v.auto_scale && !v.auto_centre {
            return ResolvedViewport {
                scale: v.scale,
                centre: v.centre,
            };
        }
        if *self.resolved_version.borrow() != self.version {
            self.resolved.borrow_mut().clear();
            *self.resolved_version.borrow_mut() = self.version;
        }
        if let Some(at) = self.resolved.borrow().get(&v.id) {
            return *at;
        }
        // With the set and the sheet, so an automatic key plan frames the sheets it outlines as
        // the painter frames them.
        let source = (self.source)(document);
        let at = resolve_plan_viewport_on_sheet(v, &source, &self.set, self.sheet);
        self.resolved.borrow_mut().insert(v.id.clone(), at);
        at
    }

    /// What is under widget point `widget`: the paper position, the viewport and, over a plan, the
    /// world point there.
    pub fn readout_at(&self, document: &Document, widget: PointF) -> SheetCursorReadout {
        let Some(sheet) = self.current_sheet() else {
            return SheetCursorReadout::default();
        };
        let resolve = |v: &Viewport| self.resolved_plan(document, v);
        sheet_cursor_readout(sheet, self.widget_to_paper(widget), Some(&resolve))
    }

    pub(crate) fn update_readout(&mut self, document: &Document, widget: PointF) {
        self.readout = self.readout_at(document, widget);
        self.cursor_paper = self.readout.on_sheet.then_some(self.readout.paper);
    }

    // ---- a drag ----------------------------------------------------------------------------

    /// The rectangle `id` is drawn at now: where a drag has it, else its own.
    pub(crate) fn live_rect_of(&self, id: &str) -> Box2 {
        let Some(v) = self.viewport(id) else {
            return Box2::default();
        };
        if self.press_moved
            && self.drag == Drag::Move
            && let Some(start) = self.start_rects.get(id)
        {
            return if self.start_rects.len() == 1 {
                self.live_rect
            } else {
                Box2::new(start.min + self.live_delta, start.max + self.live_delta)
            };
        }
        if self.press_moved && self.drag == Drag::Resize && id == self.selected {
            return self.live_rect;
        }
        v.rect
    }

    pub(crate) fn paint_dragged_content(&self, canvas: &mut dyn Canvas) {
        let moving = self.drag == Drag::Move && self.press_moved && !self.start_rects.is_empty();
        let sizing = self.drag == Drag::Resize && self.press_moved;
        if (!moving && !sizing) || self.image.is_null() {
            return;
        }
        // The last paint, in the image's own pixels: where a paper rectangle was painted in it.
        let ratio = self.ratio;
        let shift = self.origin - self.image_origin;
        let in_image = |paper: &Box2| {
            let r = self.widget_rect(paper).translated(-shift.x, -shift.y);
            IntRect::new(
                (r.x * ratio).round() as i32,
                (r.y * ratio).round() as i32,
                (r.width * ratio).round() as i32,
                (r.height * ratio).round() as i32,
            )
        };
        let from: BTreeMap<String, Box2> = if moving {
            self.start_rects.clone()
        } else {
            BTreeMap::from([(self.selected.clone(), self.start_rect)])
        };
        canvas.save();
        // Where they were, faded: the paper beneath them is not painted yet.
        for start in from.values() {
            let r = self.widget_rect(start);
            canvas.fill_rect(r, &Brush::solid(Color::rgba(255, 255, 255, 190)));
            canvas.set_pen(Pen::with(
                Color::rgb(120, 122, 128),
                1.0,
                PenStyle::Dot,
                PenCap::Square,
                PenJoin::Bevel,
            ));
            canvas.set_brush(Brush::none());
            canvas.draw_rect(r);
        }
        for (id, start) in &from {
            let to = self.widget_rect(&self.live_rect_of(id));
            if moving {
                canvas.draw_image(to, &self.image, in_image(start));
                continue;
            }
            // Sized: what it showed stays centred - a plan keeps its centre and scale - and the
            // rest is empty paper until it is painted again.
            let was = self.widget_rect(start);
            canvas.save();
            canvas.set_clip_rect(to, true);
            canvas.fill_rect(to, &Brush::solid(Color::WHITE));
            let centre = to.center() - was.center();
            canvas.draw_image(was.translated(centre.x, centre.y), &self.image, in_image(start));
            canvas.restore();
        }
        canvas.restore();
    }

    /// A drag under way abandoned, leaving everything where it was.
    pub(crate) fn cancel_drag(&mut self) {
        self.drag = Drag::None;
        self.press_moved = false;
        self.narrow_on_click = false;
        self.start_rects.clear();
        self.pan_view_id.clear();
        self.view_shift = Point2::default();
        self.guide_x = None;
        self.guide_y = None;
        self.cursor = CanvasCursor::Arrow;
    }

    // ---- the keys, and the cursor leaving --------------------------------------------------

    pub fn key_release(&mut self, key: SheetKey, auto_repeat: bool) {
        if key == SheetKey::Space && !auto_repeat {
            self.space_held = false;
            if self.drag != Drag::Pan {
                self.cursor = CanvasCursor::Arrow;
            }
        }
    }

    /// Off the canvas: the rulers' mark and the status line go blank.
    pub fn leave(&mut self) {
        self.cursor_paper = None;
        self.readout = SheetCursorReadout::default();
    }

    /// Space let go where the canvas could not hear it: no hand left behind.
    pub fn focus_out(&mut self) {
        self.space_held = false;
        if self.drag == Drag::None {
            self.cursor = CanvasCursor::Arrow;
        }
    }
}

/// The widget rectangle of a paper rectangle on `canvas`, shrunk by `inset` pixels all round:
/// what the tests count ink in, an outline drawn on its edge left out.
pub fn widget_rect_inside(canvas: &SheetCanvas, paper: &Box2, inset: f64) -> RectF {
    canvas
        .widget_rect(paper)
        .adjusted(inset, inset, -inset, -inset)
}
