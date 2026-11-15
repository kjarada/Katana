//! Several viewports at once, and the paper geometry a sheet editor works
//! with (docs/plotting.md, "Editing on the canvas";
//! include/katana/cad/plotting/viewport_edits.hpp). What a hand does on the
//! canvas - pick with a rubber band, move a group, snap to the paper grid,
//! copy and paste, read the world under the cursor - is decided here, with no
//! widget, so an agent reaches every one of them without one and a test
//! checks each one without a mouse.
//!
//! ```text
//! the paper grid   5 mm lines through the paper's origin, the same origin
//!                  the rulers count from; a drag snaps to the edges of the
//!                  other viewports first and to the grid only where no
//!                  edge was in reach
//! picking          a rubber band selects what it encloses dragged left to
//!                  right (a window), what it touches dragged right to left
//!                  (crossing), as CAD programs pick
//! the edits        move, remove, copy, paste and duplicate, each ONE
//!                  undoable step through Document::set_sheet_set, like every
//!                  edit in sheet_commands.rs
//! the world        the point of the drawing under a paper point of a plan:
//!                  the sheet painter's mapping, inverted
//! ```

use std::collections::BTreeSet;

use katana_core::error::make_error;
use katana_core::{Error, ErrorCode, Result, Status};
use katana_geometry::{Box2, Point2};

use crate::document::Document;
use crate::plotting::layout::{SnapResult, snap_rect};
use crate::plotting::sheet_set::{Sheet, SheetSet, Viewport, new_viewport_ids};

/// The step names the C++ gives by default.
pub const MOVE_VIEWPORTS: &str = "MOVE_VIEWPORTS";
pub const REMOVE_VIEWPORTS: &str = "REMOVE_VIEWPORTS";
pub const PASTE_VIEWPORTS: &str = "PASTE_VIEWPORTS";
pub const DUPLICATE_VIEWPORTS: &str = "DUPLICATE_VIEWPORTS";

// ---- the paper grid ----------------------------------------------------------------

/// The grid's spacing: fine enough to place a panel by eye, coarse enough that
/// panels placed on it line up without a guide.
pub const PAPER_GRID_MM: f64 = 5.0;

fn no_sheet(index: usize, count: usize) -> Error {
    make_error(
        ErrorCode::NotFound,
        "no sheet at that position",
        format!("{index} of {count}"),
    )
}

// `ids` as a set, each checked to be on `sheet`: the ids an edit applies to.
fn ids_on(sheet: &Sheet, ids: &[String]) -> Result<BTreeSet<String>> {
    if ids.is_empty() {
        return Err(Error::new(ErrorCode::InvalidArgument, "no viewports given"));
    }
    let mut wanted = BTreeSet::new();
    for id in ids {
        let present = sheet.viewports.iter().any(|viewport| viewport.id == *id);
        if !present {
            return Err(make_error(
                ErrorCode::NotFound,
                "no viewport with that id on the sheet",
                format!("{id} on {}", sheet.id),
            ));
        }
        wanted.insert(id.clone());
    }
    Ok(wanted)
}

// The smallest move of `own` (a rectangle's edges on one axis) that puts one
// of them on a line of the grid.
fn grid_shift(own: &[f64], grid_mm: f64) -> f64 {
    let mut best = 0.0;
    let mut found = false;
    for &edge in own {
        let shift = snap_to_grid(edge, grid_mm) - edge;
        if !found || shift.abs() < f64::abs(best) {
            best = shift;
            found = true;
        }
    }
    best
}

fn moved(rect: &Box2, delta: Point2) -> Box2 {
    Box2::new(rect.min + delta, rect.max + delta)
}

/// `value` on the nearest line of a grid of `spacing_mm` through the paper's
/// origin; unchanged when the spacing is not positive (no grid). A value
/// halfway between two lines goes to the higher one.
pub fn snap_to_grid(value: f64, spacing_mm: f64) -> f64 {
    if !(spacing_mm > 0.0) || !value.is_finite() {
        return value;
    }
    (value / spacing_mm + 0.5).floor() * spacing_mm
}

/// A rectangle being dragged, snapped. First as snap_rect snaps it: the
/// smallest move within `tolerance_mm` that puts one of its edges or its centre
/// line on an edge or centre line of the drawing area or of `others`, which
/// reports the line as a guide. Then, on an axis where nothing was in reach and
/// `grid_mm` is positive, the smallest move that puts its left or right (bottom
/// or top) edge on a grid line - so a panel of any size lands with an edge on
/// the grid, and a panel beside another lines up with it before the grid.
pub fn snap_moving_rect(
    moving: &Box2,
    drawing_area: &Box2,
    others: &[Box2],
    tolerance_mm: f64,
    grid_mm: f64,
) -> SnapResult {
    let mut result = snap_rect(moving, drawing_area, others, tolerance_mm);
    if !(grid_mm > 0.0) || moving.empty() {
        return result;
    }
    // Only where no edge or centre line was in reach: a panel lined up with
    // its neighbour stays lined up, whether or not the neighbour is on the
    // grid.
    if result.guide_x.is_none() {
        let shift = grid_shift(&[result.rect.min.x, result.rect.max.x], grid_mm);
        result.rect.min.x += shift;
        result.rect.max.x += shift;
    }
    if result.guide_y.is_none() {
        let shift = grid_shift(&[result.rect.min.y, result.rect.max.y], grid_mm);
        result.rect.min.y += shift;
        result.rect.max.y += shift;
    }
    result
}

/// One edge of a rectangle being resized, snapped the same way: to the nearest
/// of `targets` within `tolerance_mm` (the target is the guide), else to the
/// grid when `grid_mm` is positive, else left where it is.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EdgeSnap {
    pub value: f64,
    pub guide: Option<f64>,
}

pub fn snap_edge(value: f64, targets: &[f64], tolerance_mm: f64, grid_mm: f64) -> EdgeSnap {
    let mut result = EdgeSnap { value, guide: None };
    if tolerance_mm > 0.0 {
        let mut best_distance = tolerance_mm;
        for &target in targets {
            let distance = (target - value).abs();
            // The nearest wins; of two as near, the first given.
            if distance < best_distance || (distance == best_distance && result.guide.is_none()) {
                best_distance = distance;
                result.value = target;
                result.guide = Some(target);
            }
        }
    }
    if result.guide.is_none() {
        result.value = snap_to_grid(value, grid_mm);
    }
    result
}

// ---- picking -----------------------------------------------------------------------

/// How a rubber band picks: a Window takes what lies wholly inside it, a
/// Crossing everything it touches. An editor makes a band dragged left to
/// right a window and one dragged right to left a crossing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BandMode {
    Window,
    Crossing,
}

/// The ids of the placed viewports of `sheet` that `band` picks, in the
/// sheet's order (back to front).
pub fn viewports_in_band(sheet: &Sheet, band: &Box2, mode: BandMode) -> Vec<String> {
    if band.empty() {
        return Vec::new();
    }
    sheet
        .viewports
        .iter()
        .filter(|viewport| !viewport.rect.empty())
        .filter(|viewport| match mode {
            BandMode::Window => band.contains_box(viewport.rect),
            BandMode::Crossing => band.intersects(viewport.rect),
        })
        .map(|viewport| viewport.id.clone())
        .collect()
}

/// The rectangle around the viewports of `sheet` whose ids are in `ids`;
/// empty when none of them is placed on it.
pub fn viewport_bounds(sheet: &Sheet, ids: &[String]) -> Box2 {
    let mut bounds = Box2::default();
    for viewport in &sheet.viewports {
        if ids.contains(&viewport.id) {
            bounds.expand_box(viewport.rect);
        }
    }
    bounds
}

/// The placed viewport after `current` in the sheet's order (before it when
/// not `forward`), wrapping round; the first (last) when `current` is empty or
/// not on the sheet; empty when the sheet has no placed viewport. What Tab
/// steps through.
pub fn cycle_viewport(sheet: &Sheet, current: &str, forward: bool) -> String {
    let placed: Vec<&Viewport> = sheet
        .viewports
        .iter()
        .filter(|viewport| !viewport.rect.empty())
        .collect();
    let (Some(first), Some(last)) = (placed.first(), placed.last()) else {
        return String::new();
    };
    let Some(index) = placed.iter().position(|viewport| viewport.id == current) else {
        return if forward {
            first.id.clone()
        } else {
            last.id.clone()
        };
    };
    let count = placed.len();
    let next = if forward {
        (index + 1) % count
    } else {
        (index + count - 1) % count
    };
    placed[next].id.clone()
}

// ---- a plan's paper and its world --------------------------------------------------

/// The world point under paper point `paper` (mm) of a plan or key plan
/// `viewport` drawn at 1:`scale` with the world point `centre` at its
/// rectangle's centre and turned by its rotation - the sheet painter's mapping
/// inverted. `scale` and `centre` are the ones drawn: the viewport's own, or
/// what "auto" decided (resolvePlanViewport in the painter).
pub fn plan_paper_to_world(
    viewport: &Viewport,
    scale: f64,
    centre: Point2,
    paper: Point2,
) -> Point2 {
    // The painter draws world point w at middle + rotate(w - centre, -rotation)
    // * (1000 / scale) millimetres; this is that, solved for w.
    let offset = paper - viewport.rect.center();
    centre + offset.rotated(viewport.rotation) * (scale / 1000.0)
}

/// And back: where a world point is drawn on the paper.
pub fn plan_world_to_paper(
    viewport: &Viewport,
    scale: f64,
    centre: Point2,
    world: Point2,
) -> Point2 {
    viewport.rect.center() + (world - centre).rotated(-viewport.rotation) * (1000.0 / scale)
}

// ---- the edits: each ONE undoable step ----------------------------------------------
//
// Each takes the sheet by its position and the viewports by id. An id that is
// not on that sheet is refused (NotFound, naming the id), as is an empty list
// (InvalidArgument); a repeated id counts once. A refused edit changes
// nothing, and an edit that would change nothing adds no step.

/// Moves the viewports `ids` of sheet `sheet_index` by `delta_mm`, as one
/// step. A locked viewport stays where it is, as a drag leaves it; an unplaced
/// one has nowhere to move from and stays unplaced.
pub fn move_viewports(
    document: &mut Document,
    sheet_index: usize,
    ids: &[String],
    delta_mm: Point2,
    step_name: &str,
) -> Status {
    let mut set = SheetSet::clone(&document.sheet_set());
    let count = set.sheets.len();
    let Some(sheet) = set.sheets.get_mut(sheet_index) else {
        return Err(no_sheet(sheet_index, count));
    };
    if !delta_mm.is_finite() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the move is not a finite distance",
        ));
    }
    let wanted = ids_on(sheet, ids)?;
    for viewport in &mut sheet.viewports {
        if wanted.contains(&viewport.id) && !viewport.locked && !viewport.rect.empty() {
            viewport.rect = moved(&viewport.rect, delta_mm);
        }
    }
    document.set_sheet_set(&set, step_name)
}

/// Removes the viewports `ids` from sheet `sheet_index`, as one step. Locked
/// ones go too: the lock keeps a viewport where it is, not on the sheet.
pub fn remove_viewports(
    document: &mut Document,
    sheet_index: usize,
    ids: &[String],
    step_name: &str,
) -> Status {
    let mut set = SheetSet::clone(&document.sheet_set());
    let count = set.sheets.len();
    let Some(sheet) = set.sheets.get_mut(sheet_index) else {
        return Err(no_sheet(sheet_index, count));
    };
    let wanted = ids_on(sheet, ids)?;
    sheet
        .viewports
        .retain(|viewport| !wanted.contains(&viewport.id));
    document.set_sheet_set(&set, step_name)
}

/// Copies of the viewports `ids` of sheet `sheet_index` of `set`, in the
/// sheet's order (back to front), ids and all: a clipboard. Pasting gives them
/// new ids.
pub fn copy_viewports(set: &SheetSet, sheet_index: usize, ids: &[String]) -> Result<Vec<Viewport>> {
    let Some(sheet) = set.sheets.get(sheet_index) else {
        return Err(no_sheet(sheet_index, set.sheets.len()));
    };
    let wanted = ids_on(sheet, ids)?;
    Ok(sheet
        .viewports
        .iter()
        .filter(|viewport| wanted.contains(&viewport.id))
        .cloned()
        .collect())
}

/// How far a paste steps from where its copies were.
pub const PASTE_STEP_MM: f64 = 5.0;

/// Where a paste of `viewports` onto `sheet` lands, as a move from where they
/// were: the first of 0, 1, 2 ... steps of `step_mm` right and down at which no
/// pasted rectangle lies exactly on a rectangle already on the sheet. So a
/// paste onto the sheet the copies came from lands one step from the
/// originals, the next paste one step further, and a paste onto another sheet
/// (or after a cut) lands where the copies were. The C++ defaults `step_mm`
/// to PASTE_STEP_MM.
pub fn paste_offset(sheet: &Sheet, viewports: &[Viewport], step_mm: f64) -> Point2 {
    let taken = |rect: &Box2| sheet.viewports.iter().any(|on| on.rect == *rect);
    // Bounded by the sheet: each step can only be blocked by a viewport that
    // is on it, so one more step than there are viewports is always free.
    for step in 0..=sheet.viewports.len() {
        let d = step as f64 * step_mm;
        let offset = Point2::new(d, -d);
        let blocked = viewports
            .iter()
            .any(|pasted| !pasted.rect.empty() && taken(&moved(&pasted.rect, offset)));
        if !blocked {
            return offset;
        }
    }
    let d = (sheet.viewports.len() + 1) as f64 * step_mm;
    Point2::new(d, -d)
}

/// Adds `viewports` - a clipboard, from this set or another - to the front of
/// sheet `sheet_index`, as one step, with new ids (new_viewport_ids) in their
/// order, moved by `offset_mm` (paste_offset when not given). Returns the new
/// ids in the same order. InvalidArgument when there is nothing to paste.
pub fn paste_viewports(
    document: &mut Document,
    sheet_index: usize,
    viewports: Vec<Viewport>,
    offset_mm: Option<Point2>,
    step_name: &str,
) -> Result<Vec<String>> {
    if viewports.is_empty() {
        return Err(Error::new(ErrorCode::InvalidArgument, "nothing to paste"));
    }
    let mut set = SheetSet::clone(&document.sheet_set());
    let ids = paste_viewports_into(&mut set, sheet_index, viewports, offset_mm)?;
    document.set_sheet_set(&set, step_name)?;
    Ok(ids)
}

/// The same on a set, for a caller making it part of a larger step (a paste
/// into a set with no sheets adds the sheet in the same step): the C++'s
/// `pasteViewports(SheetSet&, ...)` overload.
pub fn paste_viewports_into(
    set: &mut SheetSet,
    sheet_index: usize,
    mut viewports: Vec<Viewport>,
    offset_mm: Option<Point2>,
) -> Result<Vec<String>> {
    if viewports.is_empty() {
        return Err(Error::new(ErrorCode::InvalidArgument, "nothing to paste"));
    }
    if sheet_index >= set.sheets.len() {
        return Err(no_sheet(sheet_index, set.sheets.len()));
    }
    // The C++'s value_or works its default out whether it is used or not;
    // the offset has no other effect, so working it out only when needed is
    // the same.
    let offset = offset_mm
        .unwrap_or_else(|| paste_offset(&set.sheets[sheet_index], &viewports, PASTE_STEP_MM));
    if !offset.is_finite() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the paste offset is not a finite distance",
        ));
    }
    let ids = new_viewport_ids(set, viewports.len());
    for (pasted, id) in viewports.iter_mut().zip(&ids) {
        pasted.id = id.clone();
        if !pasted.rect.empty() {
            pasted.rect = moved(&pasted.rect, offset);
        }
    }
    set.sheets[sheet_index].viewports.extend(viewports);
    Ok(ids)
}

/// Copies of the viewports `ids` of sheet `sheet_index` pasted onto the same
/// sheet (one step from the originals, paste_offset), as one step; returns the
/// copies' ids in the sheet's order of their originals.
pub fn duplicate_viewports(
    document: &mut Document,
    sheet_index: usize,
    ids: &[String],
    step_name: &str,
) -> Result<Vec<String>> {
    let copies = copy_viewports(&document.sheet_set(), sheet_index, ids)?;
    paste_viewports(document, sheet_index, copies, None, step_name)
}
