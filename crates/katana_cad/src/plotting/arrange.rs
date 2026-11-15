//! Arranging a sheet, and advice on its paper, scale and rotation
//! (docs/plotting.md, "Arranging a sheet"; include/katana/cad/plotting/arrange.hpp).
//! Pure functions on sheets and world points: the editor's Arrange menu and
//! the command line call the same ones.
//!
//! ```text
//! best_fit_rotation       the turn that shows the content at the largest scale
//!                         in a rectangle: rotating calipers over its hull
//! least_rotation_to_fit   the smallest turn at which it fits at a given scale;
//! best_standard_rotation    and the smallest turn that reaches the largest
//!                         standard scale - a drawing is turned only when that
//!                         buys a larger scale, and no further than it must be
//! suggest_paper           the smallest ISO sheet that holds it at a scale
//! suggest_scale           the largest standard scale it fits at
//! fit_paper_to_viewport   a sheet put on the paper its main view needs
//! auto_arrange            no two views overlapping, the main view first
//! align_viewports, distribute_viewports, match_scale, fit_viewport_to_content,
//! rotate_to_best_fit      the editor's Arrange commands, on ids
//! ```
//!
//! "Content" is a set of world points: the drawing's outline (drawn_outline),
//! a stretch of an alignment, a key plan's outlines (viewport_content gathers
//! what a view shows). Only its convex hull matters, so a caller may pass
//! every vertex or just the hull.
//!
//! These change a Sheet or SheetSet value; arrange_commands.rs makes each of
//! them ONE undoable step on a document.
//!
//! Rotations follow Viewport::rotation: radians counter-clockwise, the world
//! direction that runs left to right across the paper. A rotation returned
//! here is in (-90, 90] degrees, since a drawing turned half a turn needs the
//! same room upside down.
//!
//! The C++ overloads become: `suggest_paper` (content) and
//! `suggest_paper_for_extent` (a box); `suggest_scale` (a rectangle) and
//! `suggest_scale_on_sheet` (a sheet's tiling area); `viewport_content` (one
//! viewport) and `viewport_content_in_set` (a viewport of a set, where a key
//! plan's content is its live outlines).

use katana_core::error::make_error;
use katana_core::text::format_exact_real;
use katana_core::{Error, ErrorCode, Result, Status};
use katana_entity::entity_geometry::bounding_box;
use katana_entity::{Entity, EnumName, Geometry, Model};
use katana_geometry::alignment::solve_alignment;
use katana_geometry::polygon::convex_hull;
use katana_geometry::{Arc2, Box2, Point2, SpatialIndex, Vec2};
use katana_math::cpp;

use crate::drawing::construction::is_construction_layer;
use crate::hatching::resolve_hatch_pattern;
use crate::layer_overrides::LayerOverrides;
use crate::plot::{PaperSize, SHEET_SCALES, sheet_scale_at_least};
use crate::plotting::frame::BUILT_IN_FRAME_ID;
use crate::plotting::generators::{LayoutRequest, SheetTemplate, blank_sheet, smart_layout};
use crate::plotting::key_plan::{PlanPlacer, key_plan_outlines, model_placer};
use crate::plotting::layout::{
    SizeMm, TilingPreset, minimum_size, preset_cells, tiling_area, tiling_rank,
};
use crate::plotting::sheet_set::{Sheet, SheetSet, Viewport, ViewportKind, std_fixed};
use crate::selection::{is_drawn, touches_box};
use crate::spatial_query::for_each_candidate;

/// A best rotation within this of a multiple of 90 degrees is snapped to it:
/// a drawing turned 0.4 degrees gains nothing a reader would notice, and costs
/// a north arrow that is not quite up.
pub const ROTATION_SNAP_DEGREES: f64 = 1.0;

/// How much room a viewport's own fit leaves round its content: the painter's
/// automatic scale keeps 4% to spare (sheet_painter.cpp), so a rectangle sized
/// to 5% more than the content is chosen the same scale by it.
pub const FIT_SPARE: f64 = 1.05;
/// The room the painter's automatic scale leaves: what rotate_to_best_fit fits
/// inside, so the scale it writes is the one the painter will choose.
pub const AUTO_SCALE_SPARE: f64 = 1.04;

const PI: f64 = std::f64::consts::PI;
const HALF_PI: f64 = PI / 2.0;
const SNAP_RADIANS: f64 = ROTATION_SNAP_DEGREES * PI / 180.0;
// Two needed scales this close, relatively, are the same scale: they differ
// by rounding, not by anything a sheet could show.
const SCALE_TIE: f64 = 1e-9;
// Paper millimetres below which two edges are the same edge.
const PAPER_EPSILON: f64 = 1e-9;
// How far apart, at most, the points an arc or circle is traced by are.
const ARC_STEP: f64 = 2.0 * PI / 32.0;
// How far an alignment's chords may stray from it in the drawing's outline
// (and a curved hatch boundary's in draws_in), and how far apart a view's
// stretch of one is sampled, metres; a stretch is never sampled at more
// points than this.
const ALIGNMENT_CHORD_M: f64 = 0.05;
const ALIGNMENT_STEP_M: f64 = 0.5;
const MOST_ALIGNMENT_STEPS: f64 = 20000.0;
// auto_arrange's steps: the views that give way shrink by 5% at a time, the
// main view by 10% of its size at a time.
const SHRINK_STEP: f64 = 0.95;
const MAIN_SHRINK_STEP: f64 = 0.1;
const MOST_SHRINK_STEPS: i32 = 200;

/// The content turned to `rotation` in a rectangle: how big it is and the
/// scale at which it just fills the rectangle.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RotationFit {
    /// Radians, as Viewport::rotation.
    pub rotation: f64,
    /// The world point at the rectangle's centre: the middle of the content
    /// as turned, so it sits centred on the paper.
    pub centre: Point2,
    /// The content's size along the rotation (across the paper) and square to
    /// it (up the paper), metres.
    pub width: f64,
    pub height: f64,
    /// 1 : scale fills the rectangle exactly; standard_scale is the first of
    /// SHEET_SCALES at or above it (sheet_scale_at_least).
    pub scale: f64,
    pub standard_scale: f64,
}

// `angle` turned by whole half turns into (-pi/2, pi/2]: a drawing turned
// half a turn needs the same room upside down.
fn half_turn_normal(angle: f64) -> f64 {
    let mut a = cpp::remainder(angle, PI);
    if a <= -HALF_PI {
        a += PI;
    }
    a
}

// Whether `a` is nearer 0 than `b`, the positive one winning a tie: the
// rotation a person expects when two turn the drawing as well.
fn nearer_zero(a: f64, b: f64) -> bool {
    if a.abs() != b.abs() {
        return a.abs() < b.abs();
    }
    a > b
}

// The first of SHEET_SCALES at or above `scale`, a needed scale a rounding
// above a step counting as that step.
fn standard_scale_for(scale: f64) -> f64 {
    if !(scale > 0.0) {
        return 0.0;
    }
    if scale > SHEET_SCALES[SHEET_SCALES.len() - 1] * (1.0 + SCALE_TIE) {
        return scale; // beyond the ladder, as sheet_scale_at_least says
    }
    sheet_scale_at_least(scale / (1.0 + SCALE_TIE)).unwrap_or(scale)
}

// The content's hull about a point in its middle, so the millions of metres
// of a projected coordinate do not cost the digits a rotation needs.
struct Hull {
    origin: Point2,
    // Counter-clockwise, relative to origin.
    points: Vec<Point2>,
}

fn hull_of(content: &[Point2]) -> Result<Hull> {
    let mut extent = Box2::default();
    for &point in content {
        if !point.is_finite() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "a point of the content is not finite",
            ));
        }
        extent.expand(point);
    }
    if extent.empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "there is no content to fit",
        ));
    }
    let origin = extent.center();
    let relative: Vec<Point2> = content.iter().map(|&point| point - origin).collect();
    let points = convex_hull(relative);
    if points.len() < 2 {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the content is a single point and has no size to fit",
        ));
    }
    Ok(Hull { origin, points })
}

fn check_rectangle(rectangle: SizeMm) -> Status {
    if !(rectangle.width > 0.0)
        || !(rectangle.height > 0.0)
        || !rectangle.width.is_finite()
        || !rectangle.height.is_finite()
    {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the rectangle to fit has no size",
        ));
    }
    Ok(())
}

fn across(rotation: f64) -> Vec2 {
    Vec2::new(rotation.cos(), rotation.sin())
}

fn up(rotation: f64) -> Vec2 {
    Vec2::new(-rotation.sin(), rotation.cos())
}

fn fit_of(hull: &Hull, rectangle: SizeMm, rotation: f64) -> RotationFit {
    let u = across(rotation);
    let v = up(rotation);
    let mut min_u = f64::INFINITY;
    let mut max_u = -min_u;
    let mut min_v = min_u;
    let mut max_v = -min_u;
    for &point in &hull.points {
        min_u = cpp::min(min_u, point.dot(u));
        max_u = cpp::max(max_u, point.dot(u));
        min_v = cpp::min(min_v, point.dot(v));
        max_v = cpp::max(max_v, point.dot(v));
    }
    let width = max_u - min_u;
    let height = max_v - min_v;
    let scale = cpp::max(
        width * 1000.0 / rectangle.width,
        height * 1000.0 / rectangle.height,
    );
    RotationFit {
        rotation,
        centre: hull.origin + u * ((min_u + max_u) / 2.0) + v * ((min_v + max_v) / 2.0),
        width,
        height,
        scale,
        standard_scale: standard_scale_for(scale),
    }
}

// One stretch of the rotating calipers: between two orientations at which a
// hull edge lies along or square to the paper, the same pair of vertices is
// widest across the paper and the same pair up it, so the width and height
// are sinusoids of the rotation there.
#[derive(Clone, Copy, Default)]
struct Stretch {
    from: f64,
    // > from; the last stretch runs past pi/2 to wrap round.
    to: f64,
    // The widest vertex along the rotation less the narrowest.
    across: Vec2,
    // The same square to it.
    up: Vec2,
}

fn needed_scale(stretch: &Stretch, rectangle: SizeMm, rotation: f64) -> f64 {
    cpp::max(
        stretch.across.dot(across(rotation)) * 1000.0 / rectangle.width,
        stretch.up.dot(up(rotation)) * 1000.0 / rectangle.height,
    )
}

// The vertex furthest along `direction`, climbing counter-clockwise from
// `start`: the hull is convex and the direction only ever turns
// counter-clockwise, so the furthest vertex only ever moves that way.
fn furthest(hull: &[Point2], direction: Vec2, start: usize) -> usize {
    let mut at = start;
    for _ in 0..hull.len() {
        let next = (at + 1) % hull.len();
        if !(hull[next].dot(direction) > hull[at].dot(direction)) {
            break;
        }
        at = next;
    }
    at
}

fn furthest_of_all(hull: &[Point2], direction: Vec2) -> usize {
    let mut best = 0;
    for (i, point) in hull.iter().enumerate().skip(1) {
        if point.dot(direction) > hull[best].dot(direction) {
            best = i;
        }
    }
    best
}

fn calipers(hull: &[Point2]) -> Vec<Stretch> {
    // Every orientation at which an edge lies along or square to the paper,
    // in (-pi/2, pi/2].
    let mut angles: Vec<f64> = Vec::new();
    for (i, &vertex) in hull.iter().enumerate() {
        let edge = hull[(i + 1) % hull.len()] - vertex;
        let base = edge.angle() - (edge.angle() / HALF_PI).floor() * HALF_PI;
        angles.push(half_turn_normal(base));
        angles.push(half_turn_normal(base - HALF_PI));
    }
    // The angles are finite (the hull's points are), so the order is total.
    angles.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    // std::unique: each angle against the last one kept.
    angles.dedup_by(|later, kept| *later - *kept <= 1e-12);
    let mut stretches: Vec<Stretch> = Vec::new();
    let mut support = [0_usize; 4];
    for i in 0..angles.len() {
        let from = angles[i];
        let to = if i + 1 < angles.len() {
            angles[i + 1]
        } else {
            angles[0] + PI
        };
        if !(to > from) {
            continue;
        }
        let middle = (from + to) / 2.0;
        let directions = [
            across(middle),
            across(middle) * -1.0,
            up(middle),
            up(middle) * -1.0,
        ];
        for (slot, &direction) in support.iter_mut().zip(&directions) {
            *slot = if stretches.is_empty() {
                furthest_of_all(hull, direction)
            } else {
                furthest(hull, direction, *slot)
            };
        }
        stretches.push(Stretch {
            from,
            to,
            across: hull[support[0]] - hull[support[1]],
            up: hull[support[2]] - hull[support[3]],
        });
    }
    stretches
}

#[derive(Clone, Copy)]
struct Optimum {
    rotation: f64,
    scale: f64,
}

// The least scale any rotation needs, and the rotation nearest 0 that needs
// it. In a stretch the width and height are concave in the rotation (a
// cosine near its crest), so their larger is least at an end of the stretch
// or where the two ask the same scale.
fn optimum(stretches: &[Stretch], rectangle: SizeMm) -> Optimum {
    let mut best = Optimum {
        rotation: 0.0,
        scale: f64::INFINITY,
    };
    let mut consider = |stretch: &Stretch, rotation: f64| {
        let scale = needed_scale(stretch, rectangle, rotation);
        let normal = half_turn_normal(rotation);
        if scale < best.scale * (1.0 - SCALE_TIE)
            || (scale <= best.scale * (1.0 + SCALE_TIE) && nearer_zero(normal, best.rotation))
        {
            best = Optimum {
                rotation: normal,
                scale: cpp::min(scale, best.scale),
            };
        }
    };
    for stretch in stretches {
        consider(stretch, stretch.from);
        // H * (across . u) = W * (up . v): A cos t + B sin t = 0.
        let a = rectangle.height * stretch.across.x - rectangle.width * stretch.up.y;
        let b = rectangle.height * stretch.across.y + rectangle.width * stretch.up.x;
        if a == 0.0 && b == 0.0 {
            continue;
        }
        let root = (-a).atan2(b);
        for k in -2..=2 {
            let rotation = root + f64::from(k) * PI;
            if rotation > stretch.from && rotation < stretch.to {
                consider(stretch, rotation);
            }
        }
    }
    best
}

// One rotation of least_feasible's: kept when it needs no more than `limit`
// and is nearer 0 than the one kept so far.
fn consider_feasible(chosen: &mut Option<f64>, limit: f64, rotation: f64, needed: f64) {
    let normal = half_turn_normal(rotation);
    if needed <= limit && chosen.is_none_or(|kept| nearer_zero(normal, kept)) {
        *chosen = Some(normal);
    }
}

// Every rotation t at which pair . across(t + shift) = reach:
// |pair| cos(t + shift - phase) = reach.
fn roots(pair: Vec2, reach: f64, shift: f64, found: &mut dyn FnMut(f64)) {
    let length = pair.length();
    if !(length > 0.0) || reach > length {
        return;
    }
    let phase = pair.angle();
    let half = cpp::clamp(reach / length, -1.0, 1.0).acos();
    for root in [phase - shift + half, phase - shift - half] {
        for k in -2..=2 {
            found(root + 2.0 * PI * f64::from(k));
        }
    }
}

// The rotation nearest 0 at which the content needs no more than `scale`.
// The rotations that fit make a closed set, so the nearest to 0 is 0 itself
// or a rotation where the width or the height just reaches its limit: those
// are solved in each stretch, where each is a cosine.
fn least_feasible(
    hull: &Hull,
    stretches: &[Stretch],
    rectangle: SizeMm,
    scale: f64,
    best: &Optimum,
) -> Option<f64> {
    let limit = scale * (1.0 + SCALE_TIE);
    if fit_of(hull, rectangle, 0.0).scale <= limit {
        return Some(0.0);
    }
    let mut chosen: Option<f64> = None;
    if best.scale <= limit {
        consider_feasible(&mut chosen, limit, best.rotation, best.scale);
    }
    for stretch in stretches {
        let mut in_stretch = |rotation: f64| {
            if ((stretch.from - 1e-12)..=(stretch.to + 1e-12)).contains(&rotation) {
                consider_feasible(
                    &mut chosen,
                    limit,
                    rotation,
                    needed_scale(stretch, rectangle, rotation),
                );
            }
        };
        roots(
            stretch.across,
            rectangle.width * scale / 1000.0,
            0.0,
            &mut in_stretch,
        );
        roots(
            stretch.up,
            rectangle.height * scale / 1000.0,
            HALF_PI,
            &mut in_stretch,
        );
    }
    // A turn within a degree of square is squared when it still fits.
    if let Some(kept) = chosen
        && half_turn_normal(kept - HALF_PI).abs() <= SNAP_RADIANS
        && fit_of(hull, rectangle, HALF_PI).scale <= limit
    {
        chosen = Some(HALF_PI);
    }
    chosen
}

// ---- auto_arrange ----------------------------------------------------------------------

// Whether the insides of two rectangles meet; touching edges do not.
fn overlaps(a: &Box2, b: &Box2) -> bool {
    !a.empty()
        && !b.empty()
        && a.min.x < b.max.x - PAPER_EPSILON
        && b.min.x < a.max.x - PAPER_EPSILON
        && a.min.y < b.max.y - PAPER_EPSILON
        && b.min.y < a.max.y - PAPER_EPSILON
}

fn overlaps_any(rect: &Box2, others: &[Box2]) -> bool {
    others.iter().any(|other| overlaps(rect, other))
}

// `rect` moved, and cut where it is bigger, to lie inside `space`.
fn clamp_into(rect: &Box2, space: &Box2) -> Box2 {
    let width = cpp::min(rect.width(), space.width());
    let height = cpp::min(rect.height(), space.height());
    // std::clamp needs its low end at or below its high end; a rectangle as
    // wide as the space can leave space.max.x - width a rounding below
    // space.min.x.
    let x = cpp::clamp(
        rect.min.x,
        space.min.x,
        cpp::max(space.min.x, space.max.x - width),
    );
    let y = cpp::clamp(
        rect.min.y,
        space.min.y,
        cpp::max(space.min.y, space.max.y - height),
    );
    Box2::new(Point2::new(x, y), Point2::new(x + width, y + height))
}

// The free space as maximal empty rectangles, each carved round what is
// taken: the "maximal rectangles" of a bin packer. A rectangle inside another
// is dropped; of two equal ones, the first is kept.
fn carve(rooms: &mut Vec<Box2>, taken: &Box2) {
    let mut pieces: Vec<Box2> = Vec::new();
    for room in rooms.iter() {
        if !overlaps(room, taken) {
            pieces.push(*room);
            continue;
        }
        if taken.min.x > room.min.x + PAPER_EPSILON {
            pieces.push(Box2::new(room.min, Point2::new(taken.min.x, room.max.y)));
        }
        if taken.max.x < room.max.x - PAPER_EPSILON {
            pieces.push(Box2::new(Point2::new(taken.max.x, room.min.y), room.max));
        }
        if taken.min.y > room.min.y + PAPER_EPSILON {
            pieces.push(Box2::new(room.min, Point2::new(room.max.x, taken.min.y)));
        }
        if taken.max.y < room.max.y - PAPER_EPSILON {
            pieces.push(Box2::new(Point2::new(room.min.x, taken.max.y), room.max));
        }
    }
    rooms.clear();
    for (i, piece) in pieces.iter().enumerate() {
        let inside = pieces
            .iter()
            .enumerate()
            .any(|(j, other)| j != i && other.contains_box(*piece) && (other != piece || j < i));
        if !inside {
            rooms.push(*piece);
        }
    }
}

// Where a `width` x `height` rectangle goes in the free space: the top-left
// corner of the free rectangle that holds it highest up, then furthest left,
// the order a sheet is read in.
fn place_in(rooms: &[Box2], width: f64, height: f64) -> Option<Box2> {
    let mut best: Option<&Box2> = None;
    for room in rooms {
        if width > room.width() + PAPER_EPSILON || height > room.height() + PAPER_EPSILON {
            continue;
        }
        let better = match best {
            None => true,
            Some(kept) => {
                room.max.y > kept.max.y + PAPER_EPSILON
                    || (room.max.y >= kept.max.y - PAPER_EPSILON
                        && room.min.x < kept.min.x - PAPER_EPSILON)
            }
        };
        if better {
            best = Some(room);
        }
    }
    let best = best?;
    Some(Box2::new(
        Point2::new(best.min.x, best.max.y - height),
        Point2::new(best.min.x + width, best.max.y),
    ))
}

#[derive(Clone, Copy, Default)]
struct PackItem {
    // What it would like.
    size: SizeMm,
    // Never smaller than this.
    least: SizeMm,
}

// An item's size at `factor`, no larger in area than `cap` (when positive)
// and never below its least.
fn sized_at(item: &PackItem, factor: f64, cap: f64) -> SizeMm {
    let mut width = item.size.width * factor;
    let mut height = item.size.height * factor;
    if cap > 0.0 && width * height > cap {
        let k = (cap / (width * height)).sqrt();
        width *= k;
        height *= k;
    }
    SizeMm {
        width: cpp::max(width, item.least.width),
        height: cpp::max(height, item.least.height),
    }
}

// Every item placed at `factor`, or nothing when one does not fit.
// `cap_by_first`: the first item is the main view, and no later one may be
// larger in area than it is placed: how the main view stays the largest when
// it is packed with the rest.
fn pack_at(
    items: &[PackItem],
    taken: &[Box2],
    space: &Box2,
    gutter: f64,
    factor: f64,
    cap: f64,
    cap_by_first: bool,
) -> Option<Vec<Box2>> {
    let mut rooms = vec![*space];
    for taken_box in taken {
        carve(&mut rooms, &taken_box.inflated(gutter));
    }
    let mut placed: Vec<Box2> = Vec::new();
    for item in items {
        let limit = match placed.first() {
            Some(first) if cap_by_first => first.width() * first.height(),
            _ => cap,
        };
        let size = sized_at(item, factor, limit);
        let at = place_in(&rooms, size.width, size.height)?;
        carve(&mut rooms, &at.inflated(gutter));
        placed.push(at);
    }
    Some(placed)
}

// The items packed at the largest of 1, 0.95, 0.95^2 ... that fits, down to
// each one's least size.
fn pack_shrinking(
    items: &[PackItem],
    taken: &[Box2],
    space: &Box2,
    gutter: f64,
    cap: f64,
    cap_by_first: bool,
) -> Option<Vec<Box2>> {
    let mut factor = 1.0;
    for _ in 0..MOST_SHRINK_STEPS {
        if let Some(placed) = pack_at(items, taken, space, gutter, factor, cap, cap_by_first) {
            return Some(placed);
        }
        let at_least = items.iter().all(|item| {
            item.size.width * factor <= item.least.width
                && item.size.height * factor <= item.least.height
        });
        if at_least {
            break;
        }
        factor *= SHRINK_STEP;
    }
    None
}

// The preset tiling would use for `count` views, for the size an unplaced one
// would be given.
fn preset_for(count: usize) -> TilingPreset {
    match count {
        1 => TilingPreset::Full,
        2 => TilingPreset::MainRight,
        3 => TilingPreset::MainTwoRight,
        _ => TilingPreset::Quad,
    }
}

fn find_viewport(sheet: &Sheet, id: &str) -> Option<usize> {
    sheet
        .viewports
        .iter()
        .position(|viewport| viewport.id == id)
}

// The ids asked for, each once, in the order given; NotFound for one not on
// the sheet.
fn viewports_of(sheet: &Sheet, ids: &[String]) -> Result<Vec<usize>> {
    let mut found: Vec<usize> = Vec::new();
    for id in ids {
        let Some(index) = find_viewport(sheet, id) else {
            return Err(make_error(
                ErrorCode::NotFound,
                "no viewport of that id on the sheet",
                id.as_str(),
            ));
        };
        if !found.contains(&index) {
            found.push(index);
        }
    }
    Ok(found)
}

// The ids of `sheet`'s viewports whose rectangle differs from `before`, in
// the sheet's order.
fn moved_since(sheet: &Sheet, before: &[Box2]) -> Vec<String> {
    sheet
        .viewports
        .iter()
        .zip(before)
        .filter(|(viewport, was)| viewport.rect != **was)
        .map(|(viewport, _)| viewport.id.clone())
        .collect()
}

fn rects_of(sheet: &Sheet) -> Vec<Box2> {
    sheet
        .viewports
        .iter()
        .map(|viewport| viewport.rect)
        .collect()
}

fn is_section(kind: ViewportKind) -> bool {
    kind == ViewportKind::LongSection || kind == ViewportKind::CrossSections
}

fn is_plan(kind: ViewportKind) -> bool {
    kind == ViewportKind::Plan || kind == ViewportKind::KeyPlan
}

fn scale_words(scale: f64) -> String {
    // A whole denominator as a scale rule reads it, never 1:1e+06.
    if scale == scale.round() {
        format!("1:{}", std_fixed(scale, 0))
    } else {
        format!("1:{}", format_exact_real(scale))
    }
}

// A kind as a message names it.
fn kind_words(kind: ViewportKind) -> &'static str {
    match kind {
        ViewportKind::Plan => "plan",
        ViewportKind::LongSection => "long section",
        ViewportKind::CrossSections => "cross-section",
        ViewportKind::Model3D => "3D snapshot",
        ViewportKind::Legend => "legend",
        ViewportKind::Notes => "notes",
        ViewportKind::Image => "image",
        ViewportKind::KeyPlan => "key plan",
        ViewportKind::SheetIndex => "drawing register",
        ViewportKind::Revisions => "revision table",
    }
}

fn area_of(rect: &Box2) -> f64 {
    rect.width() * rect.height()
}

fn place(sheet: &mut Sheet, which: &[usize], rects: &[Box2]) {
    for (&index, &rect) in which.iter().zip(rects) {
        sheet.viewports[index].rect = rect;
    }
}

fn items_of(items: &[PackItem], which: &[usize]) -> Vec<PackItem> {
    which.iter().map(|&index| items[index]).collect()
}

// ---- Rotation -------------------------------------------------------------------------

/// The content at `rotation` in a rectangle of `rectangle` millimetres.
/// InvalidArgument for a rectangle with no size, a point that is not finite,
/// or content with no size (fewer than two distinct points).
pub fn fit_at_rotation(
    content: &[Point2],
    rectangle: SizeMm,
    rotation: f64,
) -> Result<RotationFit> {
    check_rectangle(rectangle)?;
    if !rotation.is_finite() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the rotation is not finite",
        ));
    }
    let hull = hull_of(content)?;
    Ok(fit_of(&hull, rectangle, rotation))
}

/// The rotation at which the content fills a rectangle of `rectangle`'s
/// aspect at the largest scale. Found exactly: over the hull's rotating
/// calipers the needed scale is, between two orientations where an edge lies
/// flat, the larger of two concave curves, so it is least at such an
/// orientation or where the width and the height ask the same scale (both
/// kinds are tried). Of rotations that tie, the one nearest 0 wins (the
/// positive one of two equally near); a result within ROTATION_SNAP_DEGREES of
/// a multiple of 90 degrees is snapped to it, at the scale that costs. A line
/// fits best along the rectangle's diagonal. Errors as fit_at_rotation.
pub fn best_fit_rotation(content: &[Point2], rectangle: SizeMm) -> Result<RotationFit> {
    check_rectangle(rectangle)?;
    let hull = hull_of(content)?;
    let best = optimum(&calipers(&hull.points), rectangle);
    let mut rotation = best.rotation;
    for square in [0.0, HALF_PI] {
        if half_turn_normal(rotation - square).abs() <= SNAP_RADIANS {
            rotation = square;
        }
    }
    Ok(fit_of(&hull, rectangle, rotation))
}

/// The rotation nearest 0 at which the content fits the rectangle at
/// 1 : `scale` or larger: 0 when it already does. Snapped to a multiple of 90
/// degrees within ROTATION_SNAP_DEGREES when that still fits.
/// InvalidArgument as fit_at_rotation, for a scale that is not positive, and
/// when no rotation fits (the message says the scale the best rotation needs).
pub fn least_rotation_to_fit(
    content: &[Point2],
    rectangle: SizeMm,
    scale: f64,
) -> Result<RotationFit> {
    check_rectangle(rectangle)?;
    if !(scale > 0.0) || !scale.is_finite() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the scale must be a positive number",
        ));
    }
    let hull = hull_of(content)?;
    let stretches = calipers(&hull.points);
    let best = optimum(&stretches, rectangle);
    let Some(rotation) = least_feasible(&hull, &stretches, rectangle, scale, &best) else {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!(
                "the content does not fit {} at any rotation; turned its best it needs {}",
                scale_words(scale),
                scale_words(standard_scale_for(best.scale))
            ),
        ));
    };
    Ok(fit_of(&hull, rectangle, rotation))
}

/// The rotation nearest 0 that reaches the largest STANDARD scale any
/// rotation reaches: the content is turned only when turning buys a larger
/// scale off SHEET_SCALES, and then no further than that scale needs. What
/// "Rotate to Best Fit" and the fitted generator use.
pub fn best_standard_rotation(content: &[Point2], rectangle: SizeMm) -> Result<RotationFit> {
    check_rectangle(rectangle)?;
    let hull = hull_of(content)?;
    let stretches = calipers(&hull.points);
    let best = optimum(&stretches, rectangle);
    // The best rotation needs best.scale, so its standard scale always fits
    // at that rotation at least.
    let target = standard_scale_for(best.scale);
    let rotation = least_feasible(&hull, &stretches, rectangle, target, &best);
    Ok(fit_of(&hull, rectangle, rotation.unwrap_or(best.rotation)))
}

// An arc by its ends and points on it no more than ARC_STEP apart, and
// between each two the corner of the polygon whose sides touch the arc at
// them (half-way round, radius / cos(half the step) out), so the hull holds
// the curve itself and not only its chords. The C++'s int cast of the step
// count is undefined for a sweep too large for an int; the cast here
// saturates, and there was no guard to port.
fn trace_arc(points: &mut Vec<Point2>, arc: &Arc2) {
    let steps = ((arc.sweep.abs() / ARC_STEP).ceil() as i32).max(1);
    let half = arc.sweep / f64::from(steps) / 2.0;
    let reach = arc.radius / half.cos();
    for i in 0..=steps {
        points.push(arc.point_at(f64::from(i) / f64::from(steps)));
        if i < steps {
            let angle = arc.start_angle + arc.sweep * f64::from(i) / f64::from(steps) + half;
            points.push(arc.center + Vec2::new(angle.cos(), angle.sin()) * reach);
        }
    }
}

/// The convex hull (counter-clockwise) of everything `view` draws of `model`:
/// each drawn entity by its vertices (arcs and circles by a polygon round
/// them with corners no more than 1/32 of a turn apart, so the hull holds the
/// whole curve; texts and dimensions by their boxes) and every alignment, as
/// planDrawnBounds counts it. What a plan of the drawing shows.
pub fn drawn_outline(model: &Model, view: &LayerOverrides) -> Vec<Point2> {
    let mut points: Vec<Point2> = Vec::new();
    model.entities.for_each(|entity| {
        if !is_drawn(model, entity, view) {
            return;
        }
        match &entity.geometry {
            Geometry::Point(point) => points.push(point.position),
            Geometry::Line(segment) => {
                points.push(segment.start);
                points.push(segment.end);
            }
            Geometry::Arc(arc) => trace_arc(&mut points, arc),
            Geometry::Polyline(polyline) => points.extend_from_slice(&polyline.vertices),
            Geometry::Circle(circle) => trace_arc(
                &mut points,
                &Arc2::new(circle.center, circle.radius, 0.0, 2.0 * PI),
            ),
            _ => {
                // Texts and dimensions by their boxes: small beside what they
                // label.
                let extent = bounding_box(&entity.geometry);
                if !extent.empty() {
                    points.extend([
                        extent.min,
                        Point2::new(extent.max.x, extent.min.y),
                        extent.max,
                        Point2::new(extent.min.x, extent.max.y),
                    ]);
                }
            }
        }
    });
    model.alignments.for_each(|alignment| {
        if let Ok(solved) = solve_alignment(&alignment.horizontal) {
            // Chords within ALIGNMENT_CHORD_M of the curve: a hair on paper at
            // any scale a sheet is drawn at.
            points.extend(solved.to_polyline(ALIGNMENT_CHORD_M).vertices);
        }
    });
    points.retain(|point| point.is_finite());
    convex_hull(points)
}

// Whether `entity` is a hatched closed boundary whose fill covers `point`. A
// box wholly inside a fill touches none of its edges and is still drawn on,
// and a box no edge meets is wholly in or wholly out, so its centre is the
// point to ask about.
fn fill_covers(model: &Model, entity: &Entity, point: Point2) -> bool {
    let closed = match &entity.geometry {
        Geometry::Polyline(polyline) => polyline.closed,
        Geometry::CurvePolyline(curve) => curve.closed,
        _ => false,
    };
    if !closed {
        return false;
    }
    match resolve_hatch_pattern(model, entity) {
        Some(pattern) if !pattern.draws_nothing() => {}
        _ => return false,
    }
    match &entity.geometry {
        Geometry::Polyline(polyline) => polyline.contains(point),
        Geometry::CurvePolyline(curve) => curve.to_polyline(ALIGNMENT_CHORD_M).contains(point),
        _ => false,
    }
}

/// Whether a plan of `model` with `view`'s layers hidden draws anything in
/// `area`: an entity the rule draws (construction lines aside, as
/// drawn_extent leaves them) by its geometry as a crossing box takes it
/// (touches_box), a hatched closed boundary by its fill as well, and every
/// alignment. What GENERATE asks of each tile headless (GridRequest::drawn);
/// the window adds its imagery, surfaces, meshes and road model (planDrawsIn).
/// `index`, when given, narrows the search and gives the same answer
/// (for_each_candidate).
pub fn draws_in(
    model: &Model,
    view: &LayerOverrides,
    area: &Box2,
    index: Option<&SpatialIndex>,
) -> bool {
    if area.empty() {
        return false;
    }
    let mut found = false;
    let mut scratch = Vec::new();
    for_each_candidate(model, index, *area, &mut scratch, |entity| {
        if found || !is_drawn(model, entity, view) || is_construction_layer(&entity.layer) {
            return;
        }
        found = touches_box(&entity.geometry, area) || fill_covers(model, entity, area.center());
    });
    model.alignments.for_each(|alignment| {
        if found {
            return;
        }
        if let Ok(solved) = solve_alignment(&alignment.horizontal) {
            found = touches_box(
                &Geometry::Polyline(solved.to_polyline(ALIGNMENT_CHORD_M)),
                area,
            );
        }
    });
    found
}

/// What a plan or key plan viewport shows of `model`, as the convex hull of
/// world points: its stretch of the alignment its source names (the whole
/// alignment when the chainage range is empty), else the drawing as its
/// hidden layers leave it (drawn_outline). Empty for any other kind, and when
/// there is nothing to show. The sheet editor adds the window's reference
/// layers and meshes to this.
pub fn viewport_content(model: &Model, viewport: &Viewport) -> Vec<Point2> {
    if !is_plan(viewport.kind) {
        return Vec::new();
    }
    let mut points: Vec<Point2> = Vec::new();
    let source = &viewport.source;
    let solved = if source.alignment.is_empty() {
        None
    } else {
        model
            .alignments
            .find(&source.alignment)
            .and_then(|alignment| solve_alignment(&alignment.horizontal).ok())
    };
    if let Some(solved) = &solved {
        // The stretch the view shows (the whole alignment for an empty range)
        // at points no more than ALIGNMENT_STEP_M apart, and every point where
        // one element gives way to the next.
        let mut from = solved.start_station();
        let mut to = solved.end_station();
        if source.chainage_to > source.chainage_from {
            from = cpp::max(source.chainage_from, from);
            to = cpp::min(source.chainage_to, to);
        }
        if to > from {
            let steps = cpp::clamp(
                ((to - from) / ALIGNMENT_STEP_M).ceil(),
                1.0,
                MOST_ALIGNMENT_STEPS,
            ) as i32;
            for i in 0..=steps {
                if let Some(point) =
                    solved.point_at_station(from + (to - from) * f64::from(i) / f64::from(steps))
                {
                    points.push(point);
                }
            }
            for station in solved.key_stations() {
                if station > from
                    && station < to
                    && let Some(point) = solved.point_at_station(station)
                {
                    points.push(point);
                }
            }
        }
    }
    // No alignment, or a range wholly off it, which shows none of it: the
    // painter then fits the drawing (resolvePlanViewport), and so does this.
    if points.is_empty() {
        points = drawn_outline(model, &viewport.hidden_layers);
    }
    points.retain(|point| point.is_finite());
    convex_hull(points)
}

/// The same for a viewport of `set`, where a key plan's content is what the
/// painter fits it to: the outlines of the sheets' plans, live
/// (key_plan_outlines, each automatic plan placed by `place`, by place_plan
/// over `model` when none is given), and the drawing only when the set has no
/// plan to outline. The outlines it stored when it was made are stale and not
/// counted. Any other viewport is as above.
pub fn viewport_content_in_set(
    model: &Model,
    set: &SheetSet,
    viewport: &Viewport,
    place: Option<PlanPlacer<'_>>,
) -> Vec<Point2> {
    if viewport.kind != ViewportKind::KeyPlan {
        return viewport_content(model, viewport);
    }
    let on_sheet = set
        .sheets
        .iter()
        .position(|sheet| sheet.viewports.iter().any(|each| each.id == viewport.id));
    let placer = model_placer(model);
    let chosen: PlanPlacer<'_> = match place {
        Some(given) => given,
        None => &placer,
    };
    let mut points: Vec<Point2> = Vec::new();
    for outline in key_plan_outlines(set, on_sheet.unwrap_or(set.sheets.len()), Some(chosen)) {
        points.extend(outline.corners);
    }
    points.retain(|point| point.is_finite());
    if points.is_empty() {
        // No plan to outline: the painter fits the drawing (fit_key_plan).
        return viewport_content(model, viewport);
    }
    convex_hull(points)
}

// ---- Paper and scale ------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct PaperRequest {
    /// 1 : scale.
    pub scale: f64,
    /// The content turned as a viewport turns it.
    pub rotation: f64,
    /// The frame a landscape sheet is given; empty for none. A portrait sheet
    /// has no frame (frame.rs) and its drawing area is the paper less 10 mm.
    pub frame: String,
    /// How much of the tiling area's width and height the content may use: 1
    /// for a view that fills the sheet; a main cell's share when panels sit
    /// beside it (0.64 of the width for "Main and two panels right").
    pub share_across: f64,
    pub share_up: f64,
}

impl Default for PaperRequest {
    fn default() -> Self {
        Self {
            scale: 500.0,
            rotation: 0.0,
            frame: String::from(BUILT_IN_FRAME_ID),
            share_across: 1.0,
            share_up: 1.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PaperAdvice {
    pub paper: PaperSize,
    pub landscape: bool,
    /// What the sheet's frame becomes: empty on portrait.
    pub frame: String,
    /// That sheet's tiling area.
    pub area: Box2,
    /// How much of the room the content uses: the larger of its width's and
    /// its height's share, in (0, 1].
    pub fill: f64,
}

impl Default for PaperAdvice {
    fn default() -> Self {
        Self {
            paper: PaperSize::A3,
            landscape: true,
            frame: String::new(),
            area: Box2::default(),
            fill: 0.0,
        }
    }
}

/// The smallest ISO sheet, A4 to A0 and landscape before portrait at each
/// size, whose tiling area holds the content at the request's scale and
/// rotation. InvalidArgument for a scale or share that is not positive and
/// errors as fit_at_rotation; NotFound when not even A0 holds it, saying the
/// scale at which A0 would.
pub fn suggest_paper(content: &[Point2], request: &PaperRequest) -> Result<PaperAdvice> {
    if !(request.scale > 0.0) || !request.scale.is_finite() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the scale must be a positive number",
        ));
    }
    if !(request.share_across > 0.0)
        || !(request.share_across <= 1.0)
        || !(request.share_up > 0.0)
        || !(request.share_up <= 1.0)
    {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the share of the sheet the content may use must be more than 0 and at most 1",
        ));
    }
    let fit = fit_at_rotation(
        content,
        SizeMm {
            width: 1.0,
            height: 1.0,
        },
        request.rotation,
    )?;
    let width = fit.width * 1000.0 / request.scale;
    let height = fit.height * 1000.0 / request.scale;
    let mut best_on_a0 = f64::INFINITY;
    for paper in [
        PaperSize::A4,
        PaperSize::A3,
        PaperSize::A2,
        PaperSize::A1,
        PaperSize::A0,
    ] {
        for landscape in [true, false] {
            let sheet = blank_sheet(
                &SheetTemplate {
                    paper,
                    landscape,
                    frame: request.frame.clone(),
                },
                String::new(),
            );
            let area = tiling_area(&sheet);
            let room_across = area.width() * request.share_across;
            let room_up = area.height() * request.share_up;
            if !(room_across > 0.0) || !(room_up > 0.0) {
                continue;
            }
            let fill = cpp::max(width / room_across, height / room_up);
            if fill <= 1.0 + SCALE_TIE {
                return Ok(PaperAdvice {
                    paper,
                    landscape,
                    frame: sheet.frame,
                    area,
                    fill,
                });
            }
            if paper == PaperSize::A0 {
                best_on_a0 = cpp::min(best_on_a0, request.scale * fill);
            }
        }
    }
    Err(Error::new(
        ErrorCode::NotFound,
        format!(
            "at {} the content is {} x {} mm and not even A0 holds it; A0 would at {}",
            scale_words(request.scale),
            std_fixed(width, 0),
            std_fixed(height, 0),
            scale_words(standard_scale_for(best_on_a0))
        ),
    ))
}

/// The same for a world extent, unrotated (the C++ overload taking a Box2).
pub fn suggest_paper_for_extent(extent: &Box2, request: &PaperRequest) -> Result<PaperAdvice> {
    if extent.empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "there is no content to fit",
        ));
    }
    let corners = [
        extent.min,
        Point2::new(extent.max.x, extent.min.y),
        extent.max,
        Point2::new(extent.min.x, extent.max.y),
    ];
    suggest_paper(&corners, request)
}

/// The largest standard scale (the first of SHEET_SCALES at or above what it
/// needs) at which the content, turned to `rotation`, fits `rectangle`;
/// beyond 1 : 50 000 the scale it needs, as sheet_scale_at_least gives.
/// Errors as fit_at_rotation.
pub fn suggest_scale(content: &[Point2], rectangle: &Box2, rotation: f64) -> Result<f64> {
    let fit = fit_at_rotation(
        content,
        SizeMm {
            width: rectangle.width(),
            height: rectangle.height(),
        },
        rotation,
    )?;
    Ok(fit.standard_scale)
}

/// The same for the whole of a sheet's tiling area (the C++ overload taking a
/// Sheet).
pub fn suggest_scale_on_sheet(content: &[Point2], sheet: &Sheet, rotation: f64) -> Result<f64> {
    suggest_scale(content, &tiling_area(sheet), rotation)
}

/// Puts the sheet on the advised paper: its size, orientation and frame.
/// Every placed viewport keeps its place in proportion (its rectangle mapped
/// from the old tiling area onto the new one), so a view that filled the
/// sheet fills the new one. Returns the ids of the viewports that moved, in
/// order.
pub fn apply_paper(sheet: &mut Sheet, advice: &PaperAdvice) -> Vec<String> {
    let before = rects_of(sheet);
    let from = tiling_area(sheet);
    sheet.paper = advice.paper;
    sheet.landscape = advice.landscape;
    sheet.frame = if advice.landscape {
        advice.frame.clone()
    } else {
        String::new()
    };
    let to = tiling_area(sheet);
    let map = |point: Point2| {
        Point2::new(
            to.min.x + (point.x - from.min.x) * to.width() / from.width(),
            to.min.y + (point.y - from.min.y) * to.height() / from.height(),
        )
    };
    if from != to && from.width() > 0.0 && from.height() > 0.0 {
        for viewport in &mut sheet.viewports {
            if !viewport.rect.empty() {
                viewport.rect = Box2::new(map(viewport.rect.min), map(viewport.rect.max));
            }
        }
    }
    moved_since(sheet, &before)
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PaperChange {
    pub advice: PaperAdvice,
    /// As apply_paper returns them.
    pub moved: Vec<String>,
}

/// "Choose paper for this scale": puts `sheet` on the smallest paper that
/// holds what its viewport `viewport_id` shows (`content`) at 1 : `scale`,
/// turned as the view turns it, with the painter's AUTO_SCALE_SPARE round it,
/// in the share of the tiling area the view's rectangle takes now
/// (suggest_paper, then apply_paper). The frame asked for is the sheet's own
/// on landscape paper; a portrait sheet asks for the built-in frame, which a
/// landscape suggestion then gets. The view is given `scale` and centred on
/// the content, its automatic scale and centring turned off, so the new paper
/// does not choose another. NotFound for an id not on the sheet;
/// InvalidArgument for a view that is not a plan or key plan or is not
/// placed, or a scale that is not positive; errors as suggest_paper.
pub fn fit_paper_to_viewport(
    sheet: &mut Sheet,
    viewport_id: &str,
    content: &[Point2],
    scale: f64,
) -> Result<PaperChange> {
    let Some(index) = find_viewport(sheet, viewport_id) else {
        return Err(make_error(
            ErrorCode::NotFound,
            "no viewport of that id on the sheet",
            viewport_id,
        ));
    };
    let viewport = &sheet.viewports[index];
    if !is_plan(viewport.kind) {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!(
                "the paper is chosen for a plan or key plan; this is a {} view",
                kind_words(viewport.kind)
            ),
            viewport.id.as_str(),
        ));
    }
    if viewport.rect.empty() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "the view is not placed on the paper",
            viewport.id.as_str(),
        ));
    }
    if !(scale > 0.0) || !scale.is_finite() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the scale must be a positive number",
        ));
    }
    // The share of the room the view takes now: a main view beside panels
    // keeps its share, and the panels theirs, on the new paper. The content
    // is measured with the painter's room to spare, so it does not touch the
    // view's edge.
    let area = tiling_area(sheet);
    let mut request = PaperRequest {
        scale: scale / AUTO_SCALE_SPARE,
        rotation: viewport.rotation,
        frame: if sheet.landscape {
            sheet.frame.clone()
        } else {
            String::from(BUILT_IN_FRAME_ID)
        },
        ..PaperRequest::default()
    };
    if area.width() > 0.0 && area.height() > 0.0 {
        request.share_across = cpp::clamp(viewport.rect.width() / area.width(), 0.0, 1.0);
        request.share_up = cpp::clamp(viewport.rect.height() / area.height(), 0.0, 1.0);
    }
    let advice = suggest_paper(content, &request)?;
    // Measured centred, so drawn centred.
    let fit = fit_at_rotation(
        content,
        SizeMm {
            width: 1.0,
            height: 1.0,
        },
        viewport.rotation,
    )?;
    let viewport = &mut sheet.viewports[index];
    viewport.scale = scale;
    viewport.auto_scale = false;
    viewport.centre = fit.centre;
    viewport.auto_centre = false;
    let moved = apply_paper(sheet, &advice);
    Ok(PaperChange { advice, moved })
}

// ---- Arranging viewports --------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ArrangeResult {
    /// The viewports whose rectangle changed, in the sheet's order.
    pub moved: Vec<String>,
    /// Those still overlapping another, and those still unplaced: no room for
    /// them even at their kind's minimum size. Both empty when the sheet was
    /// arranged completely.
    pub overlapping: Vec<String>,
    pub unplaced: Vec<String>,
    /// Whether the main view had to be made smaller to make room.
    pub main_shrunk: bool,
}

/// Removes the overlaps between the sheet's unlocked viewports, inside the
/// tiling area:
///
/// ```text
/// - locked viewports stay where they are, and others keep clear of them;
/// - the main view (the lowest tiling rank, ties in the sheet's order) keeps
///   its place and size;
/// - every other view that overlaps nothing, lies inside the tiling area (one
///   that strayed outside is brought in first) and is no larger than the main
///   view stays where it is;
/// - the rest, in rank order, are packed into the free space one gutter from
///   everything, each at the highest, then left-most, place it fits; when they
///   do not all fit they shrink together, in 5% steps, never below their
///   kind's minimum; when that is not enough every view but the main one is
///   packed again; and only then is the main view made smaller, from its
///   top-left corner, in 10% steps. A view that gives way is never made larger
///   than the main view.
/// ```
///
/// A main view that overlaps a locked one, or is not placed, is packed first,
/// with the rest, and none of them is made larger than it is packed. An
/// unplaced viewport is packed with the size of the cell tiling would give
/// it. A view with no room left even at its minimum stays where it was and is
/// reported; the views placed after it keep clear of it. Deterministic: the
/// same sheet is always arranged the same way, and a sheet arranged completely
/// does not change when it is arranged again. The C++'s default gutter is
/// TILING_GUTTER_MM.
pub fn auto_arrange(sheet: &mut Sheet, gutter_mm: f64) -> ArrangeResult {
    let mut result = ArrangeResult::default();
    let space = tiling_area(sheet).inflated(-gutter_mm / 2.0);
    // Inside the packing space, give or take a rounding: a tiled cell lies
    // exactly on its edge.
    let within = space.inflated(PAPER_EPSILON);
    let before = rects_of(sheet);
    let mut locked: Vec<Box2> = Vec::new();
    let mut order: Vec<usize> = Vec::new();
    for (i, viewport) in sheet.viewports.iter().enumerate() {
        if viewport.locked {
            if !viewport.rect.empty() {
                locked.push(viewport.rect);
            }
        } else {
            order.push(i);
        }
    }
    if order.is_empty() || space.empty() {
        return result;
    }
    // A stable sort, as the C++'s std::ranges::stable_sort.
    order.sort_by_key(|&i| tiling_rank(sheet.viewports[i].kind));

    // Where each view is now: brought inside when it has strayed out of the
    // drawing area; unplaced ones are sized as tiling would size them.
    let cells = preset_cells(preset_for(order.len()), &tiling_area(sheet), gutter_mm);
    let count = sheet.viewports.len();
    let mut at: Vec<Box2> = vec![Box2::default(); count];
    let mut items: Vec<PackItem> = vec![PackItem::default(); count];
    for (rank, &i) in order.iter().enumerate() {
        let viewport = &sheet.viewports[i];
        if !viewport.rect.empty() {
            at[i] = if within.contains_box(viewport.rect) {
                viewport.rect
            } else {
                clamp_into(&viewport.rect, &space)
            };
        }
        let cell = &cells[rank.min(cells.len() - 1)];
        let size = if at[i].empty() {
            SizeMm {
                width: cell.width(),
                height: cell.height(),
            }
        } else {
            SizeMm {
                width: at[i].width(),
                height: at[i].height(),
            }
        };
        let minimum = minimum_size(viewport.kind);
        items[i] = PackItem {
            size,
            least: SizeMm {
                width: cpp::min(minimum.width, size.width),
                height: cpp::min(minimum.height, size.height),
            },
        };
    }
    let main = order[0];

    // Keep every view that overlaps nothing kept before it (the main view
    // first) and pack the rest into what is left. A main view in the way of
    // a locked one, or not placed, is packed first, with the rest, so it
    // still gets the most room. A view larger than the main one gives way
    // too: the main view is the largest on the sheet.
    let mut taken: Vec<Box2> = locked.clone();
    let mut kept: Vec<usize> = Vec::new();
    let mut displaced: Vec<usize> = Vec::new();
    let main_kept = !at[main].empty() && !overlaps_any(&at[main], &locked);
    let main_area = if main_kept { area_of(&at[main]) } else { 0.0 };
    for &i in &order {
        if main_kept
            && !at[i].empty()
            && !overlaps_any(&at[i], &taken)
            && (i == main || area_of(&at[i]) <= main_area + PAPER_EPSILON)
        {
            taken.push(at[i]);
            kept.push(i);
        } else {
            displaced.push(i);
        }
    }
    let cap = main_area;
    for &i in &kept {
        sheet.viewports[i].rect = at[i];
    }
    let mut arranged = displaced.is_empty();
    if !arranged
        && main_kept
        && let Some(packed) = pack_shrinking(
            &items_of(&items, &displaced),
            &taken,
            &space,
            gutter_mm,
            cap,
            false,
        )
    {
        place(sheet, &displaced, &packed);
        arranged = true;
    }
    // Then every view but the main one, packed again round it.
    let others: Vec<usize> = order[usize::from(main_kept)..].to_vec();
    let mut round: Vec<Box2> = locked.clone();
    if main_kept {
        round.push(at[main]);
    }
    if !arranged
        && let Some(packed) = pack_shrinking(
            &items_of(&items, &others),
            &round,
            &space,
            gutter_mm,
            cap,
            !main_kept,
        )
    {
        place(sheet, &others, &packed);
        arranged = true;
    }
    // Then the main view made smaller from its top-left corner, a tenth at a
    // time, never below its least size.
    if !arranged && main_kept {
        let item = items[main];
        let mut step = 1;
        while !arranged && step < 10 {
            let size = sized_at(&item, 1.0 - MAIN_SHRINK_STEP * f64::from(step), 0.0);
            let smaller = Box2::new(
                Point2::new(at[main].min.x, at[main].max.y - size.height),
                Point2::new(at[main].min.x + size.width, at[main].max.y),
            );
            let mut around_smaller = locked.clone();
            around_smaller.push(smaller);
            if let Some(packed) = pack_shrinking(
                &items_of(&items, &others),
                &around_smaller,
                &space,
                gutter_mm,
                area_of(&smaller),
                false,
            ) {
                sheet.viewports[main].rect = smaller;
                place(sheet, &others, &packed);
                result.main_shrunk = true;
                arranged = true;
            }
            if size.width <= item.least.width && size.height <= item.least.height {
                break;
            }
            step += 1;
        }
    }
    // Last, the views that were in the way are placed one at a time where
    // each still fits, shrinking in the same steps; the rest stay where they
    // were, and are reported.
    if !arranged {
        for &i in &kept {
            sheet.viewports[i].rect = at[i];
        }
        let mut rooms = vec![space];
        for taken_box in &taken {
            carve(&mut rooms, &taken_box.inflated(gutter_mm));
        }
        for &i in &displaced {
            sheet.viewports[i].rect = if at[i].empty() { before[i] } else { at[i] };
            // No larger than the main view as it now stands.
            let main_rect = sheet.viewports[main].rect;
            let limit = if i == main {
                0.0
            } else if main_kept {
                cap
            } else {
                area_of(&main_rect)
            };
            let mut placed = false;
            let mut factor = 1.0;
            for _ in 0..MOST_SHRINK_STEPS {
                let size = sized_at(&items[i], factor, limit);
                if let Some(spot) = place_in(&rooms, size.width, size.height) {
                    sheet.viewports[i].rect = spot;
                    carve(&mut rooms, &spot.inflated(gutter_mm));
                    placed = true;
                    break;
                }
                if size.width <= items[i].least.width && size.height <= items[i].least.height {
                    break;
                }
                factor *= SHRINK_STEP;
            }
            // One with no room stays where it was, and is reported; the views
            // placed after it keep clear of it rather than land on it.
            if !placed && !sheet.viewports[i].rect.empty() {
                let stays = sheet.viewports[i].rect.inflated(gutter_mm);
                carve(&mut rooms, &stays);
            }
        }
    }

    result.moved = moved_since(sheet, &before);
    for (i, viewport) in sheet.viewports.iter().enumerate() {
        if viewport.locked {
            continue;
        }
        if viewport.rect.empty() {
            result.unplaced.push(viewport.id.clone());
            continue;
        }
        let covered = sheet
            .viewports
            .iter()
            .enumerate()
            .any(|(j, other)| j != i && overlaps(&viewport.rect, &other.rect));
        if covered {
            result.overlapping.push(viewport.id.clone());
        }
    }
    result
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AlignEdge {
    Left,
    Right,
    Top,
    Bottom,
    /// Centres on one vertical line.
    HorizontalCentre,
    /// Centres on one horizontal line.
    VerticalCentre,
}

/// "left", "right", "top", "bottom", "hcentre", "vcentre": the C++
/// `toString(AlignEdge)`.
impl EnumName for AlignEdge {
    fn name(self) -> &'static str {
        match self {
            AlignEdge::Left => "left",
            AlignEdge::Right => "right",
            AlignEdge::Top => "top",
            AlignEdge::Bottom => "bottom",
            AlignEdge::HorizontalCentre => "hcentre",
            AlignEdge::VerticalCentre => "vcentre",
        }
    }
}

pub fn align_edge_from(name: &str) -> Option<AlignEdge> {
    [
        AlignEdge::Left,
        AlignEdge::Right,
        AlignEdge::Top,
        AlignEdge::Bottom,
        AlignEdge::HorizontalCentre,
        AlignEdge::VerticalCentre,
    ]
    .into_iter()
    .find(|edge| edge.name() == name)
}

/// Lines up the viewports `ids` of `sheet` on the outermost of their edges
/// (the leftmost left edge, the highest top...) or on the middle of the box
/// round them. One viewport alone is aligned to the tiling area. A locked
/// viewport counts where it is and does not move; an unplaced one is left
/// out. Returns the ids that moved, in the sheet's order. NotFound for an id
/// not on the sheet; InvalidArgument when none of them is placed.
pub fn align_viewports(sheet: &mut Sheet, ids: &[String], edge: AlignEdge) -> Result<Vec<String>> {
    let found = viewports_of(sheet, ids)?;
    let placed: Vec<usize> = found
        .iter()
        .copied()
        .filter(|&index| !sheet.viewports[index].rect.empty())
        .collect();
    if placed.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "there is nothing to align: none of the views is placed on the paper",
        ));
    }
    // The box the edges are taken from: the views' own, or the tiling area
    // for one alone. One asked for alone aligns to the tiling area; two asked
    // for of which one is not placed is not "one alone", and the placed one
    // keeps its place.
    let mut reference = Box2::default();
    if found.len() == 1 {
        reference = tiling_area(sheet);
    } else {
        for &index in &placed {
            reference.expand_box(sheet.viewports[index].rect);
        }
    }
    let before = rects_of(sheet);
    for &index in &placed {
        let viewport = &mut sheet.viewports[index];
        if viewport.locked {
            continue;
        }
        let rect = viewport.rect;
        let mut shift = Vec2::default();
        match edge {
            AlignEdge::Left => shift.x = reference.min.x - rect.min.x,
            AlignEdge::Right => shift.x = reference.max.x - rect.max.x,
            AlignEdge::Top => shift.y = reference.max.y - rect.max.y,
            AlignEdge::Bottom => shift.y = reference.min.y - rect.min.y,
            AlignEdge::HorizontalCentre => shift.x = reference.center().x - rect.center().x,
            AlignEdge::VerticalCentre => shift.y = reference.center().y - rect.center().y,
        }
        viewport.rect = Box2::new(rect.min + shift, rect.max + shift);
    }
    Ok(moved_since(sheet, &before))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DistributeAxis {
    Horizontal,
    Vertical,
}

/// "horizontal", "vertical": the C++ `toString(DistributeAxis)`.
impl EnumName for DistributeAxis {
    fn name(self) -> &'static str {
        if self == DistributeAxis::Horizontal {
            "horizontal"
        } else {
            "vertical"
        }
    }
}

pub fn distribute_axis_from(name: &str) -> Option<DistributeAxis> {
    match name {
        "horizontal" => Some(DistributeAxis::Horizontal),
        "vertical" => Some(DistributeAxis::Vertical),
        _ => None,
    }
}

/// Spaces the viewports `ids` so the gaps between them are equal: ordered by
/// their left (bottom) edges, the first and last stay and the others move
/// between them. Locked and unplaced viewports are left out. Returns the ids
/// that moved. NotFound for an id not on the sheet; InvalidArgument for fewer
/// than three that can move.
pub fn distribute_viewports(
    sheet: &mut Sheet,
    ids: &[String],
    axis: DistributeAxis,
) -> Result<Vec<String>> {
    let found = viewports_of(sheet, ids)?;
    let mut moving: Vec<usize> = found
        .iter()
        .copied()
        .filter(|&index| {
            let viewport = &sheet.viewports[index];
            !viewport.rect.empty() && !viewport.locked
        })
        .collect();
    if moving.len() < 3 {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!(
                "spacing views out needs three or more placed, unlocked views; there {} {}",
                if moving.len() == 1 { "is" } else { "are" },
                moving.len()
            ),
        ));
    }
    let horizontal = axis == DistributeAxis::Horizontal;
    let low = |rect: Box2| if horizontal { rect.min.x } else { rect.min.y };
    let high = |rect: Box2| if horizontal { rect.max.x } else { rect.max.y };
    // Ties keep the sheet's order, whatever order the ids came in, so the
    // same views always space out the same way. (The C++ sorts the pointers
    // first, which is the sheet's order.)
    moving.sort_unstable();
    let rects = rects_of(sheet);
    let less = |a: usize, b: usize| {
        low(rects[a]) < low(rects[b])
            || (low(rects[a]) == low(rects[b]) && high(rects[a]) < high(rects[b]))
    };
    moving.sort_by(|&a, &b| {
        if less(a, b) {
            std::cmp::Ordering::Less
        } else if less(b, a) {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Equal
        }
    });
    let first = rects[moving[0]];
    let last = rects[moving[moving.len() - 1]];
    let middle = &moving[1..moving.len() - 1];
    let mut inside = 0.0;
    for &index in middle {
        inside += high(rects[index]) - low(rects[index]);
    }
    let gap = (low(last) - high(first) - inside) / (moving.len() - 1) as f64;
    // Wider together than the room between the first and the last, equal
    // gaps would be negative: the views pushed into each other. Refused, and
    // nothing moves.
    if gap < -PAPER_EPSILON {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!(
                "the views between the first and the last are {} mm {} together, and there is \
                 {} mm between those two: spaced out they would overlap; move the outer ones \
                 apart first",
                std_fixed(inside, 1),
                if horizontal { "wide" } else { "high" },
                std_fixed(low(last) - high(first), 1)
            ),
        ));
    }
    let before = rects;
    let mut next = high(first) + gap;
    for &index in middle {
        let rect = sheet.viewports[index].rect;
        let shift = next - low(rect);
        let delta = if horizontal {
            Vec2::new(shift, 0.0)
        } else {
            Vec2::new(0.0, shift)
        };
        let moved = Box2::new(rect.min + delta, rect.max + delta);
        sheet.viewports[index].rect = moved;
        next = high(moved) + gap;
    }
    Ok(moved_since(sheet, &before))
}

/// Whether a viewport of this kind is drawn to a scale: a plan, a key plan, a
/// long section or cross sections.
pub fn has_scale(kind: ViewportKind) -> bool {
    is_plan(kind) || is_section(kind)
}

fn find_in_set(set: &SheetSet, id: &str) -> Option<(usize, usize)> {
    set.sheets
        .iter()
        .enumerate()
        .find_map(|(s, sheet)| find_viewport(sheet, id).map(|v| (s, v)))
}

/// Gives the viewports `ids`, on any sheet of `set`, the scale of `from_id`
/// (`from_scale` when given, which is how a caller passes the scale an
/// automatic viewport is drawn at) and turns their automatic scale off. A
/// section matched to a section takes its vertical exaggeration too.
/// Viewports with no scale (has_scale) are left out. `from_exaggeration` is
/// the exaggeration an automatic section `from_id` is drawn at, when the
/// caller knows it; else its stored one. Returns the ids that changed.
/// NotFound for an unknown id; InvalidArgument when `from_id` has no scale or
/// the scale is not positive.
pub fn match_scale(
    set: &mut SheetSet,
    ids: &[String],
    from_id: &str,
    from_scale: Option<f64>,
    from_exaggeration: Option<f64>,
) -> Result<Vec<String>> {
    let Some((from_sheet, from_viewport)) = find_in_set(set, from_id) else {
        return Err(make_error(
            ErrorCode::NotFound,
            "no viewport of that id to take the scale from",
            from_id,
        ));
    };
    let from = &set.sheets[from_sheet].viewports[from_viewport];
    if !has_scale(from.kind) {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!("a {} view has no scale to match", kind_words(from.kind)),
            from_id,
        ));
    }
    let scale = from_scale.unwrap_or(from.scale);
    if !(scale > 0.0) || !scale.is_finite() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the scale must be a positive number",
        ));
    }
    let from_kind = from.kind;
    let exaggeration = from_exaggeration.unwrap_or(from.vertical_exaggeration);
    let mut targets: Vec<(usize, usize)> = Vec::new();
    for id in ids {
        let Some(found) = find_in_set(set, id) else {
            return Err(make_error(
                ErrorCode::NotFound,
                "no viewport of that id",
                id.as_str(),
            ));
        };
        targets.push(found);
    }
    let mut changed: Vec<String> = Vec::new();
    for (s, v) in targets {
        let viewport = &mut set.sheets[s].viewports[v];
        if viewport.id == from_id || !has_scale(viewport.kind) {
            continue;
        }
        let was = viewport.clone();
        viewport.scale = scale;
        viewport.auto_scale = false;
        if is_section(from_kind) && is_section(viewport.kind) {
            viewport.vertical_exaggeration = exaggeration;
        }
        if *viewport != was && !changed.contains(&viewport.id) {
            changed.push(viewport.id.clone());
        }
    }
    Ok(changed)
}

/// Sizes a viewport to its content at its scale: the rectangle grows or
/// shrinks about its centre until the content, with FIT_SPARE round it, just
/// fits, and the view is centred on the content. Never smaller than the
/// kind's minimum; kept inside `drawing_area`, which may leave it smaller than
/// the content. A plan or key plan measures the content turned to its
/// rotation; a section measures (chainage or offset, level) points, the level
/// exaggerated. The scale is kept and the centre set, so automatic scale and
/// centring are turned off: the caller gives an automatic view the scale it
/// is drawn at first. InvalidArgument for another kind, an unplaced viewport
/// or content with no size.
pub fn fit_viewport_to_content(
    viewport: &mut Viewport,
    content: &[Point2],
    drawing_area: &Box2,
) -> Status {
    if !has_scale(viewport.kind) {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!(
                "a {} view is not drawn to a scale, so it has no size to fit",
                kind_words(viewport.kind)
            ),
            viewport.id.as_str(),
        ));
    }
    if viewport.rect.empty() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "the view is not placed on the paper",
            viewport.id.as_str(),
        ));
    }
    if !(viewport.scale > 0.0) || !viewport.scale.is_finite() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "the view has no scale",
            viewport.id.as_str(),
        ));
    }
    let section = is_section(viewport.kind);
    let exaggeration = if section && viewport.vertical_exaggeration > 0.0 {
        viewport.vertical_exaggeration
    } else {
        1.0
    };
    let fit = fit_at_rotation(
        content,
        SizeMm {
            width: 1.0,
            height: 1.0,
        },
        if section { 0.0 } else { viewport.rotation },
    )?;
    let minimum = minimum_size(viewport.kind);
    let mut width = cpp::max(
        fit.width * 1000.0 / viewport.scale * FIT_SPARE,
        minimum.width,
    );
    let mut height = cpp::max(
        fit.height * 1000.0 / viewport.scale * exaggeration * FIT_SPARE,
        minimum.height,
    );
    if !drawing_area.empty() {
        width = cpp::min(width, drawing_area.width());
        height = cpp::min(height, drawing_area.height());
    }
    let middle = viewport.rect.center();
    let mut rect = Box2::new(
        Point2::new(middle.x - width / 2.0, middle.y - height / 2.0),
        Point2::new(middle.x + width / 2.0, middle.y + height / 2.0),
    );
    if !drawing_area.empty() {
        rect = clamp_into(&rect, drawing_area);
    }
    viewport.rect = rect;
    viewport.centre = fit.centre;
    viewport.auto_scale = false;
    viewport.auto_centre = false;
    Ok(())
}

/// Turns a plan or key plan to the rotation that shows its content at the
/// largest standard scale inside its rectangle (best_standard_rotation,
/// leaving the painter's AUTO_SCALE_SPARE), centres it on the content and
/// gives it that scale, turning automatic scale and centring off: the
/// painter's automatic fit measures the drawing's box, which a turned drawing
/// overfills. The rectangle does not change. Returns the fit.
/// InvalidArgument for another kind or an unplaced viewport, and errors as
/// fit_at_rotation.
pub fn rotate_to_best_fit(viewport: &mut Viewport, content: &[Point2]) -> Result<RotationFit> {
    if !is_plan(viewport.kind) {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!(
                "only a plan or key plan is turned to fit; this is a {} view",
                kind_words(viewport.kind)
            ),
            viewport.id.as_str(),
        ));
    }
    if viewport.rect.empty() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "the view is not placed on the paper",
            viewport.id.as_str(),
        ));
    }
    let fit = best_standard_rotation(
        content,
        SizeMm {
            width: viewport.rect.width() / AUTO_SCALE_SPARE,
            height: viewport.rect.height() / AUTO_SCALE_SPARE,
        },
    )?;
    viewport.rotation = fit.rotation;
    viewport.centre = fit.centre;
    viewport.scale = fit.standard_scale;
    viewport.auto_scale = false;
    viewport.auto_centre = false;
    Ok(fit)
}

// The first plan of an area on `sheet`: a plan with no alignment.
fn area_plan(sheet: &Sheet) -> Option<usize> {
    sheet.viewports.iter().position(|viewport| {
        viewport.kind == ViewportKind::Plan && viewport.source.alignment.is_empty()
    })
}

/// smart_layout for a plan of an area, with the drawing turned to fill the
/// sheet. `content` is what the plan shows (drawn_outline).
///
/// ```text
/// - An automatic scale: the plan is turned when that buys a larger standard
///   scale (best_standard_rotation), and left square otherwise.
/// - A fixed scale that needs more than one sheet square (tiles, or the 3D
///   view and legend on a sheet of their own) is tried turned on one sheet,
///   beside its panels (least_rotation_to_fit); the tiles stay when it does
///   not fit.
/// ```
///
/// InvalidArgument for a request with no plan area or with an alignment's
/// plan or sections (a strip already follows its alignment); and errors as
/// smart_layout. Content with no size is not turned. `skipped` is
/// smart_layout's, for the layout returned.
pub fn smart_layout_rotated(
    model: &Model,
    request: &LayoutRequest<'_>,
    content: &[Point2],
    mut skipped: Option<&mut Vec<String>>,
) -> Result<Vec<Sheet>> {
    if request.plan_area.is_none() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "turning the drawing to fill the sheet needs a plan of an area",
        ));
    }
    if request.plan_along_alignment || request.long_section || request.cross_section_interval > 0.0
    {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "only a plan of an area is turned to fill the sheet; a strip already follows its \
             alignment",
        ));
    }
    // What the square layout left out, taken back if the turned one is kept.
    let said = skipped.as_ref().map_or(0, |list| list.len());
    let mut sheets = smart_layout(model, request, skipped.as_deref_mut())?;
    if hull_of(content).is_err() {
        return Ok(sheets);
    }
    if !(request.scale > 0.0) {
        // Automatic: turned when that buys a larger standard scale.
        let Some(index) = sheets.first().and_then(area_plan) else {
            return Ok(sheets);
        };
        if sheets[0].viewports[index].rect.empty() {
            return Ok(sheets);
        }
        let plan = &mut sheets[0].viewports[index];
        let fit = best_standard_rotation(
            content,
            SizeMm {
                width: plan.rect.width(),
                height: plan.rect.height(),
            },
        );
        if let Ok(fit) = fit
            && fit.rotation != 0.0
            && fit.standard_scale < plan.scale
        {
            plan.rotation = fit.rotation;
            plan.centre = fit.centre;
            plan.scale = fit.standard_scale;
        }
        return Ok(sheets);
    }
    if sheets.len() <= 1 {
        return Ok(sheets); // it fits square at the scale asked
    }
    // At a fixed scale: on one sheet, turned, when it fits there.
    let automatic = LayoutRequest {
        scale: 0.0,
        ..request.clone()
    };
    let Ok(mut compact) = smart_layout(model, &automatic, None) else {
        return Ok(sheets);
    };
    if compact.len() != 1 {
        return Ok(sheets);
    }
    let Some(index) = area_plan(&compact[0]) else {
        return Ok(sheets);
    };
    if compact[0].viewports[index].rect.empty() {
        return Ok(sheets);
    }
    let plan = &mut compact[0].viewports[index];
    let Ok(fit) = least_rotation_to_fit(
        content,
        SizeMm {
            width: plan.rect.width(),
            height: plan.rect.height(),
        },
        request.scale,
    ) else {
        return Ok(sheets);
    };
    plan.rotation = fit.rotation;
    plan.centre = fit.centre;
    plan.scale = request.scale;
    if let Some(list) = skipped {
        list.truncate(said);
    }
    Ok(compact)
}
