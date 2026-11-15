//! A key plan, live (docs/plotting.md, "The coordinate grid and the live key
//! plan"; include/katana/cad/plotting/key_plan.hpp).
//!
//! A key plan shows where every sheet's plan lies on the ground, each outline
//! numbered. Its outlines are worked out from the sheets as they are when it
//! is drawn, not stored when the key plan was made, so a sheet added, removed,
//! reordered, rescaled or panned shows at once, numbered as the set numbers it
//! now. The plans on the key plan's own sheet are flagged, for the painter to
//! shade: "you are here".
//!
//! Pure functions on the sheet set; the painter only draws what they return.
//!
//! The C++ overloads of `placePlan` are `place_plan` (one viewport over a
//! model) and `place_plan_on_sheet` (a viewport of a set, where an automatic
//! key plan frames every outline).

use katana_entity::Model;
use katana_geometry::alignment::solve_alignment;
use katana_geometry::{Box2, Point2};
use katana_math::cpp;

use crate::layer_overrides::LayerOverrides;
use crate::plot::sheet_scale_at_least;
use crate::plotting::plan_grid::{PlanPlacement, plan_footprint, stored_placement};
use crate::plotting::sheet_set::{SheetSet, Viewport, ViewportKind, format_sheet_number};
use crate::selection::drawn_extent;

/// One plan viewport's ground on a key plan.
#[derive(Clone, Debug, PartialEq)]
pub struct KeyPlanOutline {
    pub sheet_id: String,
    pub sheet_index: usize,
    pub viewport_id: String,
    /// The sheet's number as it prints now: its typed override, else the set's
    /// numbering (printed_sheet_number).
    pub label: String,
    /// The ground under the plan's rectangle, world coordinates, counter-
    /// clockwise from the corner under its bottom-left (plan_footprint).
    pub corners: Vec<Point2>,
    /// On the key plan's own sheet: "you are here".
    pub current: bool,
    /// False for a plan whose centre lies inside a larger outline of the same
    /// sheet (an inset of it), whose number is written there already.
    pub labelled: bool,
}

impl Default for KeyPlanOutline {
    fn default() -> Self {
        Self {
            sheet_id: String::new(),
            sheet_index: 0,
            viewport_id: String::new(),
            label: String::new(),
            corners: Vec::new(),
            current: false,
            labelled: true,
        }
    }
}

/// Decides where an automatic plan viewport's ground is (the Qt painter's
/// resolvePlanViewport). The C++ `PlanPlacer` is a std::function, empty for
/// "none"; here it is a borrowed closure, and None is the empty one.
pub type PlanPlacer<'a> = &'a dyn Fn(&Viewport) -> PlanPlacement;

// Twice the signed area of a ring: positive counter-clockwise.
fn double_area(ring: &[Point2]) -> f64 {
    let mut sum = 0.0;
    for (i, a) in ring.iter().enumerate() {
        let b = &ring[(i + 1) % ring.len()];
        sum += a.x * b.y - b.x * a.y;
    }
    sum
}

// Whether `p` is inside the convex ring `ring` (a footprint, counter-clockwise).
fn inside_convex(ring: &[Point2], p: Point2) -> bool {
    if ring.len() < 3 {
        return false;
    }
    let sense = if double_area(ring) < 0.0 { -1.0 } else { 1.0 };
    for (i, a) in ring.iter().enumerate() {
        let b = &ring[(i + 1) % ring.len()];
        if sense * ((b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x)) < 0.0 {
            return false;
        }
    }
    true
}

fn centroid_of(ring: &[Point2]) -> Point2 {
    let mut sum = Point2::default();
    for &p in ring {
        sum += p;
    }
    sum * (1.0 / ring.len() as f64)
}

// What a plan of `model` draws with `layers` hidden, as the painter bounds it:
// the entities (drawn_extent) and every alignment, which is drawn but is no
// entity, to within a metre of its curves.
fn drawing_extent(model: &Model, layers: &LayerOverrides) -> Box2 {
    let mut extent = drawn_extent(model, layers);
    model.alignments.for_each(|alignment| {
        if let Ok(solved) = solve_alignment(&alignment.horizontal) {
            for &vertex in &solved.to_polyline(1.0).vertices {
                extent.expand(vertex);
            }
        }
    });
    extent
}

/// The number sheet `index` prints: its own "sheet_number" field when it has
/// one, else the set's numbering pattern (format_sheet_number). Empty for an
/// index past the end.
pub fn printed_sheet_number(set: &SheetSet, index: usize) -> String {
    let Some(sheet) = set.sheets.get(index) else {
        return String::new();
    };
    if let Some(typed) = sheet.fields.get("sheet_number") {
        return typed.clone();
    }
    format_sheet_number(
        &set.numbering,
        index + 1,
        set.sheets.len(),
        &set.defaults.set_number,
    )
}

/// Every placed plan viewport (ViewportKind::Plan; key plans and sections are
/// not outlined) on every sheet of `set`, in sheet order and then back to
/// front, those on sheet `sheet_index` flagged current. `place` decides each
/// automatic viewport's scale and centre; without it, and for a fixed
/// viewport, the stored ones are used. A viewport whose scale is not positive
/// and finite is left out.
#[allow(
    clippy::needless_range_loop,
    reason = "Each outline of a sheet is compared with every other by position, as the C++ does."
)]
pub fn key_plan_outlines(
    set: &SheetSet,
    sheet_index: usize,
    place: Option<PlanPlacer<'_>>,
) -> Vec<KeyPlanOutline> {
    let mut outlines: Vec<KeyPlanOutline> = Vec::new();
    for (i, sheet) in set.sheets.iter().enumerate() {
        let number = printed_sheet_number(set, i);
        let first = outlines.len();
        for viewport in &sheet.viewports {
            if viewport.kind != ViewportKind::Plan || viewport.rect.empty() {
                continue;
            }
            let automatic = viewport.auto_scale || viewport.auto_centre;
            let at = match place {
                Some(place) if automatic => place(viewport),
                _ => stored_placement(viewport),
            };
            let corners = plan_footprint(viewport, &at);
            if corners.is_empty() || !corners.iter().all(|p| p.x.is_finite() && p.y.is_finite()) {
                continue;
            }
            outlines.push(KeyPlanOutline {
                sheet_id: sheet.id.clone(),
                sheet_index: i,
                viewport_id: viewport.id.clone(),
                label: number.clone(),
                corners,
                current: i == sheet_index,
                labelled: true,
            });
        }
        // A plan inside a larger plan of its own sheet (a detail at a larger
        // scale, an inset) takes the number from it rather than repeating it.
        for a in first..outlines.len() {
            let middle = centroid_of(&outlines[a].corners);
            let area = double_area(&outlines[a].corners).abs();
            for b in first..outlines.len() {
                if b != a
                    && double_area(&outlines[b].corners).abs() > area
                    && inside_convex(&outlines[b].corners, middle)
                {
                    outlines[a].labelled = false;
                    break;
                }
            }
        }
    }
    outlines
}

/// Where an automatic plan viewport goes to show `points`, by the rule the
/// painter applies to every automatic plan: auto_centre centres their extent
/// as the viewport is turned; auto_scale takes the largest scale of
/// SHEET_SCALES at which they fit with 4% to spare. What is not automatic is
/// kept, and so is everything when there are no points.
pub fn fit_plan_placement(viewport: &Viewport, points: &[Point2]) -> PlanPlacement {
    let mut at = stored_placement(viewport);
    if points.is_empty() || viewport.rect.empty() {
        return at;
    }
    if viewport.auto_centre {
        let mut extent = Box2::default();
        for &p in points {
            extent.expand(p.rotated(-viewport.rotation));
        }
        at.centre = extent.center().rotated(viewport.rotation);
    }
    if viewport.auto_scale {
        let mut half_w = 0.0;
        let mut half_h = 0.0;
        for &p in points {
            let d = (p - at.centre).rotated(-viewport.rotation);
            half_w = cpp::max(half_w, d.x.abs());
            half_h = cpp::max(half_h, d.y.abs());
        }
        // 4% to spare, so nothing touches the viewport's edge.
        let needed = cpp::max(
            2.0 * half_w * 1000.0 / viewport.rect.width(),
            2.0 * half_h * 1000.0 / viewport.rect.height(),
        ) * 1.04;
        if needed > 0.0
            && let Ok(scale) = sheet_scale_at_least(needed)
        {
            at.scale = scale;
        }
    }
    at
}

/// Where an automatic key plan goes: fitted to the union of the outlines, so
/// no sheet falls off the edge and none is shrunk by drawing far away. With
/// no outline at all, fitted to `drawing` instead (the drawing's extent;
/// empty for none).
pub fn fit_key_plan(
    key_plan: &Viewport,
    outlines: &[KeyPlanOutline],
    drawing: &Box2,
) -> PlanPlacement {
    let mut points: Vec<Point2> = Vec::new();
    for outline in outlines {
        points.extend_from_slice(&outline.corners);
    }
    // The sheets are what a key plan is for: it frames them, and the drawing
    // only when there is no sheet to frame, so a stray entity far off (a
    // survey mark, a datum) cannot shrink every outline to a speck.
    if points.is_empty() && !drawing.empty() {
        points.extend([
            drawing.min,
            Point2::new(drawing.max.x, drawing.min.y),
            drawing.max,
            Point2::new(drawing.min.x, drawing.max.y),
        ]);
    }
    fit_plan_placement(key_plan, &points)
}

// ---- the same, headless, over a model ---------------------------------------------
//
// The painter places an automatic plan over everything the window shows
// (resolvePlanViewport in the Qt layer): the drawing and also the imagery,
// point clouds and meshes only the application holds. These place it over the
// model alone, by the same rule, so a command line or an agent with no window
// gets the same outlines, key plan and grid for a drawing of entities and
// alignments.

/// Where plan viewport `viewport` shows `model`: its stretch of its alignment
/// (Viewport::source: from/to chainage, the whole alignment when they are
/// equal), else what the model draws with the viewport's hidden layers hidden
/// (drawn_extent, and the alignments), fitted by fit_plan_placement. A fixed
/// viewport, an unplaced one, and one with nothing to fit keep their stored
/// scale and centre.
pub fn place_plan(model: &Model, viewport: &Viewport) -> PlanPlacement {
    if (!viewport.auto_scale && !viewport.auto_centre) || viewport.rect.empty() {
        return stored_placement(viewport);
    }
    // What the viewport shows: its stretch of an alignment, sampled as the
    // painter samples it, or the drawing.
    let mut points: Vec<Point2> = Vec::new();
    let from = &viewport.source;
    if !from.alignment.is_empty()
        && let Some(alignment) = model.alignments.find(&from.alignment)
        && let Ok(solved) = solve_alignment(&alignment.horizontal)
    {
        let mut a = from.chainage_from;
        let mut b = from.chainage_to;
        if !(b > a) {
            a = solved.start_station();
            b = solved.end_station();
        }
        for i in 0..=64 {
            if let Some(p) = solved.point_at_station(a + (b - a) * f64::from(i) / 64.0) {
                points.push(p);
            }
        }
    }
    if points.is_empty() {
        let extent = drawing_extent(model, &viewport.hidden_layers);
        if extent.empty() {
            return stored_placement(viewport);
        }
        points = vec![
            extent.min,
            extent.max,
            Point2::new(extent.min.x, extent.max.y),
            Point2::new(extent.max.x, extent.min.y),
        ];
    }
    fit_plan_placement(viewport, &points)
}

/// The same for a viewport on sheet `sheet_index` of `set`: an automatic key
/// plan is fitted to every outline (key_plan_outlines, each automatic plan
/// placed by place_plan), or to the drawing when there is none (fit_key_plan).
/// Any other viewport is placed as above.
pub fn place_plan_on_sheet(
    model: &Model,
    set: &SheetSet,
    sheet_index: usize,
    viewport: &Viewport,
) -> PlanPlacement {
    if viewport.kind != ViewportKind::KeyPlan
        || (!viewport.auto_scale && !viewport.auto_centre)
        || viewport.rect.empty()
    {
        return place_plan(model, viewport);
    }
    let placer = model_placer(model);
    let outlines = key_plan_outlines(set, sheet_index, Some(&placer));
    // The drawing is only the fallback for a set with no plan to frame.
    let drawing = if outlines.is_empty() {
        drawing_extent(model, &viewport.hidden_layers)
    } else {
        Box2::default()
    };
    fit_key_plan(viewport, &outlines, &drawing)
}

/// place_plan over `model`, as key_plan_outlines takes it (borrow the closure
/// to pass it). The closure borrows the model.
pub fn model_placer(model: &Model) -> impl Fn(&Viewport) -> PlanPlacement + '_ {
    move |viewport: &Viewport| place_plan(model, viewport)
}
