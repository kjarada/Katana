//! Smart sheet generators (docs/plotting.md;
//! include/katana/cad/plotting/generators.hpp): each turns a simple request
//! into sheets, deterministically - the same request gives the same sheets,
//! byte for byte, so a generated set can be tested and regenerated without
//! surprises.
//!
//! ```text
//! fit_to_sheet            an extent on one sheet at the largest scale that fits
//! grid_sheets             an area in tiles at a chosen scale, with overlap,
//!                         match lines and a key plan, leaving out the tiles
//!                         with nothing drawn in them
//! strip_sheets            an alignment in strips rotated to its bearing, with
//!                         match lines at chainages
//! cross_section_sheets    cross sections in rows and columns at one scale and
//!                         exaggeration, in chainage order, onto as many sheets
//!                         as it takes
//! sheets_from_plot_frames one sheet per imported plot frame
//! smart_layout            "show me this" - composes the above with the presets
//! ```
//!
//! None of them touches a Document: they return sheets, and the caller adds
//! them in one undoable step (sheet_commands.rs).

use std::collections::BTreeMap;

use katana_core::error::make_error;
use katana_core::text::format_exact_real;
use katana_core::{Error, ErrorCode, Result};
use katana_entity::{Geometry, Model, PropertyMap, PropertyValue};
use katana_geometry::alignment::solve_alignment;
use katana_geometry::{Box2, Point2, Polyline2, SolvedAlignment, Vec2};
use katana_math::cpp;
use katana_math::numerics::tolerance;

use crate::plot::{PaperSize, paper_dimensions, sheet_scale_at_least};
use crate::plotting::frame::BUILT_IN_FRAME_ID;
use crate::plotting::layout::{
    TILING_GUTTER_MM, TilingPreset, preset_cells, tile_viewports, tiling_area,
};
use crate::plotting::section_fit::fit_exaggeration;
use crate::plotting::sheet_set::{
    Sheet, SheetSet, Viewport, ViewportKind, WorldMark, WorldMarkKind, drawing_area, new_sheet_ids,
    new_viewport_ids, std_fixed,
};
use crate::section::{SectionOptions, SectionSurfaceInput, extract_section, section_stations};

/// The paper every generated sheet is on. A3 landscape with the built-in frame
/// by default, as the frame itself is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SheetTemplate {
    pub paper: PaperSize,
    pub landscape: bool,
    pub frame: String,
}

impl Default for SheetTemplate {
    fn default() -> Self {
        Self {
            paper: PaperSize::A3,
            landscape: true,
            frame: String::from(BUILT_IN_FRAME_ID),
        }
    }
}

/// Whether anything is drawn in a world box. What the tiling asks of each tile,
/// so that a tile over empty ground is left out: plotting::drawsIn over the
/// drawing headless; in the window, everything its plan view draws.
pub type DrawnTest<'a> = &'a dyn Fn(&Box2) -> bool;

#[derive(Clone)]
pub struct GridRequest<'a> {
    /// World.
    pub area: Box2,
    /// 1 : scale.
    pub scale: f64,
    /// How much neighbouring tiles share, in metres.
    pub overlap_m: f64,
    /// A first sheet showing every tile, numbered.
    pub key_plan: bool,
    pub paper: SheetTemplate,
    /// Given: only the tiles it says something is drawn in are kept. Not
    /// given: every tile over the area.
    pub drawn: Option<DrawnTest<'a>>,
}

impl Default for GridRequest<'_> {
    fn default() -> Self {
        Self {
            area: Box2::default(),
            scale: 500.0,
            overlap_m: 0.0,
            key_plan: true,
            paper: SheetTemplate::default(),
            drawn: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct StripRequest {
    pub scale: f64,
    pub overlap_m: f64,
    /// The chainage range to cover; the whole alignment when not given.
    pub from_chainage: Option<f64>,
    pub to_chainage: Option<f64>,
    pub key_plan: bool,
    pub paper: SheetTemplate,
    /// The paper rectangle each strip's plan fills: the tiling area when not
    /// given; the top cell of "Main and panel below" when a long section goes
    /// under it (smart_layout).
    pub plan_rect: Option<Box2>,
}

impl Default for StripRequest {
    fn default() -> Self {
        Self {
            scale: 500.0,
            overlap_m: 0.0,
            from_chainage: None,
            to_chainage: None,
            key_plan: false,
            paper: SheetTemplate::default(),
            plan_rect: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct CrossSectionRequest<'a> {
    /// Every so many metres of chainage, ends included.
    pub interval: f64,
    /// Or exactly these chainages.
    pub stations: Vec<f64>,
    /// Each section runs this far either side.
    pub half_width: f64,
    pub rows: usize,
    pub columns: usize,
    /// 0: the largest standard scale the widest fits.
    pub scale: f64,
    /// 0: the largest of SECTION_EXAGGERATIONS that fits the deepest section
    /// (1 without surfaces).
    pub exaggeration: f64,
    /// Surfaces to sample, to centre each section on its ground and choose the
    /// exaggeration; without any, each section is centred when it is drawn.
    pub surfaces: Vec<SectionSurfaceInput<'a>>,
    pub paper: SheetTemplate,
}

impl Default for CrossSectionRequest<'_> {
    fn default() -> Self {
        Self {
            interval: 20.0,
            stations: Vec::new(),
            half_width: 20.0,
            rows: 4,
            columns: 2,
            scale: 0.0,
            exaggeration: 0.0,
            surfaces: Vec::new(),
            paper: SheetTemplate::default(),
        }
    }
}

/// What to show; smart_layout decides how.
#[derive(Clone)]
pub struct LayoutRequest<'a> {
    /// A plan of this area.
    pub plan_area: Option<Box2>,
    /// An alignment in the model, for the three below.
    pub alignment: String,
    /// A plan following the alignment.
    pub plan_along_alignment: bool,
    pub long_section: bool,
    /// Cross sections every so many metres; 0: none.
    pub cross_section_interval: f64,
    pub cross_section_half_width: f64,
    pub model3d: bool,
    pub legend: bool,
    /// 0: auto.
    pub scale: f64,
    pub paper: SheetTemplate,
    /// Where a plan of an area spills onto tiles at a fixed scale, only the
    /// tiles this says something is drawn in are kept (GridRequest::drawn).
    /// Not given: every tile.
    pub drawn: Option<DrawnTest<'a>>,
}

impl Default for LayoutRequest<'_> {
    fn default() -> Self {
        Self {
            plan_area: None,
            alignment: String::new(),
            plan_along_alignment: false,
            long_section: false,
            cross_section_interval: 0.0,
            cross_section_half_width: 20.0,
            model3d: false,
            legend: false,
            scale: 0.0,
            paper: SheetTemplate::default(),
            drawn: None,
        }
    }
}

// A long section's usual exaggeration when nothing says otherwise: H 1:500
// V 1:50, the convention road and pipe long sections are drawn at.
const LONG_SECTION_EXAGGERATION: f64 = 10.0;
// How much of a cross-section cell the section may fill: the rest is its
// title and the offset and level labels under it.
const SECTION_FILL: f64 = 0.8;
// How much of a strip's height the alignment may use; the rest keeps a curve
// off the viewport's edge, where its labels would be cut.
const STRIP_FILL: f64 = 0.9;
// Two paper sizes this close (mm) are the same size.
const PAPER_MATCH_MM: f64 = 2.0;

fn metres_across(millimetres: f64, scale: f64) -> f64 {
    millimetres * scale / 1000.0
}

fn plan_viewport(rect: Box2, scale: f64, centre: Point2) -> Viewport {
    Viewport {
        kind: ViewportKind::Plan,
        rect,
        scale,
        centre,
        north_arrow: true,
        scale_bar: true,
        ..Viewport::default()
    }
}

// Ids for a generator's own output: s1.., vp1.., in order. prepare_for_append
// renumbers them when the sheets join a set.
fn number_sheets(sheets: &mut [Sheet]) {
    let mut viewport_number = 0;
    for (i, sheet) in sheets.iter_mut().enumerate() {
        if sheet.id.is_empty() {
            sheet.id = format!("s{}", i + 1);
        }
        for viewport in &mut sheet.viewports {
            viewport_number += 1;
            viewport.id = format!("vp{viewport_number}");
        }
    }
}

// A rectangle in world coordinates: `centre`, `width` along `rotation`,
// `height` across it. Counter-clockwise from the bottom-left.
fn world_rectangle(centre: Point2, width: f64, height: f64, rotation: f64) -> Vec<Point2> {
    let along = Vec2::new(rotation.cos(), rotation.sin());
    let up = Vec2::new(-along.y, along.x);
    let a = along * (width / 2.0);
    let u = up * (height / 2.0);
    vec![
        centre - a - u,
        centre + a - u,
        centre + a + u,
        centre - a + u,
    ]
}

// A key-plan sheet: every sheet's outline, numbered, fitted on one sheet.
fn key_plan_sheet(
    paper: &SheetTemplate,
    sheets: &[Sheet],
    outlines: &[Vec<Point2>],
) -> Result<Sheet> {
    let mut extent = Box2::default();
    for outline in outlines {
        for &point in outline {
            extent.expand(point);
        }
    }
    let mut sheet = fit_to_sheet(&extent, paper)?.remove(0);
    sheet.name = String::from("KEY PLAN");
    let key = &mut sheet.viewports[0];
    key.kind = ViewportKind::KeyPlan;
    for (outline, keyed) in outlines.iter().zip(sheets) {
        key.marks.push(WorldMark {
            kind: WorldMarkKind::SheetOutline,
            points: outline.clone(),
            label: String::new(),
            sheet: keyed.id.clone(),
        });
    }
    Ok(sheet)
}

fn solved_alignment(model: &Model, name: &str) -> Result<SolvedAlignment> {
    let Some(alignment) = model.alignments.find(name) else {
        return Err(make_error(
            ErrorCode::NotFound,
            "no alignment of that name",
            name,
        ));
    };
    solve_alignment(&alignment.horizontal)
}

fn number_property(properties: &PropertyMap, key: &str) -> Option<f64> {
    match properties.get(key)? {
        PropertyValue::Real(real) => Some(*real),
        PropertyValue::Integer(integer) => Some(*integer as f64),
        _ => None,
    }
}

// The ISO A size `width` x `height` millimetres is, either way round.
fn iso_paper(width: f64, height: f64) -> Option<(PaperSize, bool)> {
    let landscape = width >= height;
    [
        PaperSize::A0,
        PaperSize::A1,
        PaperSize::A2,
        PaperSize::A3,
        PaperSize::A4,
    ]
    .into_iter()
    .find(|&paper| {
        let size = paper_dimensions(paper, landscape);
        (size.width_mm - width).abs() <= PAPER_MATCH_MM
            && (size.height_mm - height).abs() <= PAPER_MATCH_MM
    })
    .map(|paper| (paper, landscape))
}

fn intersection(a: &Box2, b: &Box2) -> Box2 {
    Box2::new(
        Point2::new(cpp::max(a.min.x, b.min.x), cpp::max(a.min.y, b.min.y)),
        Point2::new(cpp::min(a.max.x, b.max.x), cpp::min(a.max.y, b.max.y)),
    )
}

// Appends a generator's output to `out`, renumbered after what is there.
fn append(out: &mut Vec<Sheet>, mut more: Vec<Sheet>) {
    // Moved into a set and back rather than copied: a long road's layout
    // appends to a hundred sheets.
    let so_far = SheetSet {
        sheets: std::mem::take(out),
        ..SheetSet::default()
    };
    prepare_for_append(&so_far, &mut more);
    *out = so_far.sheets;
    out.extend(more);
}

// The point at `station` of an alignment the station is known to be on; the
// C++ dereferences the optional unchecked.
fn point_at(alignment: &SolvedAlignment, station: f64) -> Point2 {
    alignment.point_at_station(station).unwrap_or_default()
}

fn direction_at(alignment: &SolvedAlignment, station: f64) -> f64 {
    alignment.direction_at_station(station).unwrap_or_default()
}

/// A blank sheet of `paper` named `name`.
pub fn blank_sheet(paper: &SheetTemplate, name: String) -> Sheet {
    Sheet {
        name,
        paper: paper.paper,
        landscape: paper.landscape,
        // A portrait sheet has no frame (frame.rs); say so rather than name
        // one it cannot have.
        frame: if paper.landscape {
            paper.frame.clone()
        } else {
            String::new()
        },
        ..Sheet::default()
    }
}

/// One sheet with one plan viewport filling the tiling area, centred on
/// `extent`, at the largest standard scale (SHEET_SCALES) at which all of it
/// fits. InvalidArgument for an empty extent (a single point has nothing to
/// scale to; a line along an axis does, and fits).
pub fn fit_to_sheet(extent: &Box2, paper: &SheetTemplate) -> Result<Vec<Sheet>> {
    if extent.empty()
        || !extent.width().is_finite()
        || !extent.height().is_finite()
        || (!(extent.width() > 0.0) && !(extent.height() > 0.0))
    {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "there is no extent to fit on a sheet",
        ));
    }
    let mut sheet = blank_sheet(paper, String::from("PLAN"));
    let area = tiling_area(&sheet);
    // The denominator at which each dimension just fits: metres to
    // millimetres, over the millimetres available.
    let needed = cpp::max(
        extent.width() * 1000.0 / area.width(),
        extent.height() * 1000.0 / area.height(),
    );
    let scale = sheet_scale_at_least(needed)?;
    sheet
        .viewports
        .push(plan_viewport(area, scale, extent.center()));
    let mut sheets = vec![sheet];
    number_sheets(&mut sheets);
    Ok(sheets)
}

/// Tiles covering `area` at the scale, centred on it, numbered in reading
/// order (rows from the top, left to right). With `drawn`, a tile with nothing
/// drawn in it is left out - the others keep their places on the grid - and
/// `skipped`, when given, says how many were. Each tile's plan carries a match
/// line in the middle of each overlap it shares with a tile kept, labelled
/// with the sheet across it; the key plan outlines the tiles kept.
/// InvalidArgument for an empty area, a non-positive scale, or an overlap as
/// large as a tile; NotFound when `drawn` finds nothing in any tile.
pub fn grid_sheets(
    request: &GridRequest<'_>,
    skipped: Option<&mut Vec<String>>,
) -> Result<Vec<Sheet>> {
    let area = &request.area;
    if area.empty() || !(area.width() > 0.0) || !(area.height() > 0.0) {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the area to tile has no size",
        ));
    }
    if !(request.scale > 0.0) || !request.scale.is_finite() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the scale must be positive",
        ));
    }
    let rect = tiling_area(&blank_sheet(&request.paper, String::new()));
    let tile_w = metres_across(rect.width(), request.scale);
    let tile_h = metres_across(rect.height(), request.scale);
    let overlap = request.overlap_m;
    if !(overlap >= 0.0) || !(overlap < cpp::min(tile_w, tile_h)) {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "the overlap must be at least zero and less than a tile",
            format!(
                "{} m against tiles of {} x {} m",
                format_exact_real(overlap),
                format_exact_real(tile_w),
                format_exact_real(tile_h)
            ),
        ));
    }
    let step_x = tile_w - overlap;
    let step_y = tile_h - overlap;
    // Tiles needed along each axis; the 1e-9 keeps an area that fits exactly
    // from being given a sliver of a tile by rounding. The C++'s cast of a
    // count past the range of a size_t is undefined; this one saturates.
    let count = |length: f64, tile: f64, step: f64, shared: f64| -> usize {
        if length <= tile {
            1
        } else {
            ((length - shared) / step - 1e-9).ceil() as usize
        }
    };
    let columns = count(area.width(), tile_w, step_x, overlap);
    let rows = count(area.height(), tile_h, step_y, overlap);
    // The tiles cover a little more than the area; centre them on it.
    let left0 = area.center().x - (columns as f64 * step_x + overlap) / 2.0;
    let top0 = area.center().y + (rows as f64 * step_y + overlap) / 2.0;

    // The ground each tile's plan shows, overlaps and all.
    let tile_at = |row: usize, column: usize| {
        let left = left0 + column as f64 * step_x;
        let top = top0 - row as f64 * step_y;
        Box2::new(
            Point2::new(left, top - tile_h),
            Point2::new(left + tile_w, top),
        )
    };
    // Each tile's number among those kept, in reading order; 0 for one left
    // out. A tile left out keeps its place, so the tiles either side of it
    // still meet their other neighbours on the same lines.
    let mut number = vec![0_usize; rows * columns];
    let mut kept = 0;
    for row in 0..rows {
        for column in 0..columns {
            if request
                .drawn
                .is_none_or(|drawn| drawn(&tile_at(row, column)))
            {
                kept += 1;
                number[row * columns + column] = kept;
            }
        }
    }
    if kept == 0 {
        return Err(make_error(
            ErrorCode::NotFound,
            "nothing is drawn in any tile over the area",
            format!(
                "{} tiles of {} x {} m",
                rows * columns,
                format_exact_real(tile_w),
                format_exact_real(tile_h)
            ),
        ));
    }
    if let Some(skipped) = skipped
        && kept < rows * columns
    {
        skipped.push(format!(
            "{} of {} tiles, with nothing drawn in them",
            rows * columns - kept,
            rows * columns
        ));
    }

    let mut tiles = Vec::new();
    let mut outlines = Vec::new();
    let first = if request.key_plan { 2 } else { 1 }; // the key plan is sheet 1
    // The sheet tile (row, column) is; empty for one left out.
    let id_of = |row: usize, column: usize| -> String {
        let n = number[row * columns + column];
        if n == 0 {
            String::new()
        } else {
            format!("s{}", first + n - 1)
        }
    };
    for row in 0..rows {
        for column in 0..columns {
            let n = number[row * columns + column];
            if n == 0 {
                continue;
            }
            let tile = tile_at(row, column);
            let mut sheet = blank_sheet(&request.paper, format!("PLAN TILE {n}"));
            sheet.id = id_of(row, column);
            let mut plan = plan_viewport(rect, request.scale, tile.center());
            // A match line in the middle of each shared strip, facing the
            // neighbour across it; none towards a tile left out, which has no
            // sheet to lead to.
            let mut match_line = |a: Point2, b: Point2, neighbour: String| {
                if !neighbour.is_empty() {
                    plan.marks.push(WorldMark {
                        kind: WorldMarkKind::MatchLine,
                        points: vec![a, b],
                        label: String::from("MATCH LINE"),
                        sheet: neighbour,
                    });
                }
            };
            if column + 1 < columns {
                let x = tile.max.x - overlap / 2.0;
                match_line(
                    Point2::new(x, tile.min.y),
                    Point2::new(x, tile.max.y),
                    id_of(row, column + 1),
                );
            }
            if column > 0 {
                let x = tile.min.x + overlap / 2.0;
                match_line(
                    Point2::new(x, tile.min.y),
                    Point2::new(x, tile.max.y),
                    id_of(row, column - 1),
                );
            }
            if row > 0 {
                let y = tile.max.y - overlap / 2.0;
                match_line(
                    Point2::new(tile.min.x, y),
                    Point2::new(tile.max.x, y),
                    id_of(row - 1, column),
                );
            }
            if row + 1 < rows {
                let y = tile.min.y + overlap / 2.0;
                match_line(
                    Point2::new(tile.min.x, y),
                    Point2::new(tile.max.x, y),
                    id_of(row + 1, column),
                );
            }
            sheet.viewports.push(plan);
            outlines.push(world_rectangle(tile.center(), tile_w, tile_h, 0.0));
            tiles.push(sheet);
        }
    }
    let mut sheets = Vec::new();
    if request.key_plan {
        let mut key = key_plan_sheet(&request.paper, &tiles, &outlines)?;
        key.id = String::from("s1");
        sheets.push(key);
    }
    sheets.extend(tiles);
    number_sheets(&mut sheets);
    Ok(sheets)
}

// A strip of an alignment: its chainages, and where its plan sits.
#[derive(Clone, Copy)]
struct Strip {
    from: f64,
    to: f64,
    centre: Point2,
    rotation: f64,
}

/// Strips along `alignment` (named `name` in the viewports' source), each a
/// plan rotated so the alignment runs left to right, advancing by the tile's
/// width less the overlap - shortened where a curve would leave the strip -
/// with match lines across the alignment at each change of sheet.
pub fn strip_sheets(
    alignment: &SolvedAlignment,
    name: &str,
    request: &StripRequest,
) -> Result<Vec<Sheet>> {
    if !(request.scale > 0.0) || !request.scale.is_finite() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the scale must be positive",
        ));
    }
    let rect = request
        .plan_rect
        .unwrap_or_else(|| tiling_area(&blank_sheet(&request.paper, String::new())));
    let strip_w = metres_across(rect.width(), request.scale);
    let strip_h = metres_across(rect.height(), request.scale);
    let overlap = request.overlap_m;
    if !(overlap >= 0.0) || !(overlap < strip_w) {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the overlap must be at least zero and less than a strip",
        ));
    }
    let from = cpp::max(
        request.from_chainage.unwrap_or(alignment.start_station()),
        alignment.start_station(),
    );
    let to = cpp::min(
        request.to_chainage.unwrap_or(alignment.end_station()),
        alignment.end_station(),
    );
    if !(to > from) {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "the chainage range is empty",
            format!("{} to {}", format_exact_real(from), format_exact_real(to)),
        ));
    }
    let usable = strip_w - overlap;
    // Sample spacing along the alignment for the fit test: a hundredth of the
    // strip's smaller side, so no bend between samples can leave it unseen.
    let spacing = cpp::min(strip_w, strip_h) / 100.0;

    // Whether [a, b] fits in one strip, and where the strip then sits: along
    // the chord from a to b, centred on the alignment's extent across it.
    let place = |a: f64, b: f64| -> Option<Strip> {
        let start = point_at(alignment, a);
        let end = point_at(alignment, b);
        let chord = end - start;
        let rotation = if chord.length() > 1e-9 {
            chord.angle()
        } else {
            direction_at(alignment, (a + b) / 2.0)
        };
        let along = Vec2::new(rotation.cos(), rotation.sin());
        let up = Vec2::new(-along.y, along.x);
        let mut along_min = 0.0;
        let mut along_max = 0.0;
        let mut up_min = 0.0;
        let mut up_max = 0.0;
        // The C++'s cast of a count past the range of a size_t is undefined;
        // this one saturates.
        let samples = ((b - a) / spacing).ceil() as usize;
        for i in 0..=samples {
            let station = if i == samples {
                b
            } else {
                a + (b - a) * i as f64 / samples as f64
            };
            let offset = point_at(alignment, station) - start;
            along_min = cpp::min(along_min, offset.dot(along));
            along_max = cpp::max(along_max, offset.dot(along));
            up_min = cpp::min(up_min, offset.dot(up));
            up_max = cpp::max(up_max, offset.dot(up));
        }
        if along_max - along_min > usable + 1e-9 || up_max - up_min > strip_h * STRIP_FILL {
            return None;
        }
        let centre =
            start + along * ((along_min + along_max) / 2.0) + up * ((up_min + up_max) / 2.0);
        Some(Strip {
            from: a,
            to: b,
            centre,
            rotation,
        })
    };

    let mut strips = Vec::new();
    let mut at = from;
    while to - at > 1e-6 {
        let mut end = cpp::min(at + usable, to);
        let mut strip = place(at, end);
        // Shorter until it fits: a curve tighter than the strip is tall
        // bends out of it before the strip's length is used. A tenth at a
        // time, and never below a twentieth of a strip - a hairpin that
        // fits nowhere is drawn in short strips rather than not at all.
        let mut attempt = 0;
        while strip.is_none() && attempt < 30 {
            let shorter = at + (end - at) * 0.9;
            if shorter - at < usable / 20.0 {
                break;
            }
            end = shorter;
            strip = place(at, end);
            attempt += 1;
        }
        let strip = strip.unwrap_or_else(|| Strip {
            from: at,
            to: end,
            centre: point_at(alignment, (at + end) / 2.0),
            rotation: direction_at(alignment, (at + end) / 2.0),
        });
        strips.push(strip);
        at = end;
    }

    let mut sheets = Vec::new();
    let mut outlines = Vec::new();
    let first = if request.key_plan { 2 } else { 1 };
    let id_of = |index: usize| format!("s{}", first + index);
    for (i, strip) in strips.iter().enumerate() {
        let mut sheet = blank_sheet(
            &request.paper,
            format!(
                "{name} CH {} TO {}",
                std_fixed(strip.from, 3),
                std_fixed(strip.to, 3)
            ),
        );
        sheet.id = id_of(i);
        let mut plan = plan_viewport(rect, request.scale, strip.centre);
        plan.rotation = strip.rotation;
        plan.source.alignment = String::from(name);
        plan.source.chainage_from = strip.from;
        plan.source.chainage_to = strip.to;
        // Match lines square across the alignment where one sheet hands over
        // to the next, as long as the strip is tall.
        let mut match_at = |chainage: f64, neighbour: String| {
            let left = alignment.point_at_station_offset(chainage, strip_h / 2.0);
            let right = alignment.point_at_station_offset(chainage, -strip_h / 2.0);
            if let (Some(left), Some(right)) = (left, right) {
                plan.marks.push(WorldMark {
                    kind: WorldMarkKind::MatchLine,
                    points: vec![right, left],
                    label: format!("MATCH LINE CH {}", std_fixed(chainage, 3)),
                    sheet: neighbour,
                });
            }
        };
        if i > 0 {
            match_at(strip.from, id_of(i - 1));
        }
        if i + 1 < strips.len() {
            match_at(strip.to, id_of(i + 1));
        }
        sheet.viewports.push(plan);
        outlines.push(world_rectangle(
            strip.centre,
            strip_w,
            strip_h,
            strip.rotation,
        ));
        sheets.push(sheet);
    }
    if request.key_plan {
        let mut key = key_plan_sheet(&request.paper, &sheets, &outlines)?;
        key.id = String::from("s1");
        sheets.insert(0, key);
    }
    number_sheets(&mut sheets);
    Ok(sheets)
}

// A cross section's chainage, and its ground's levels when it was sampled.
#[derive(Clone, Copy)]
struct Cut {
    chainage: f64,
    low: Option<f64>,
    high: Option<f64>,
}

/// Cross sections of `alignment` at the requested chainages, rows x columns
/// per sheet, in chainage order down each column and then across, all at one
/// scale and exaggeration, onto as many sheets as they need.
pub fn cross_section_sheets(
    alignment: &SolvedAlignment,
    name: &str,
    request: &CrossSectionRequest<'_>,
) -> Result<Vec<Sheet>> {
    if request.rows == 0 || request.columns == 0 {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "a sheet needs at least one row and column",
        ));
    }
    if !(request.half_width > 0.0) {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "a cross section needs a width",
        ));
    }
    // Chainages, and each section's line, from the alignment itself - not
    // from a distance along its chorded polyline, which is a little shorter
    // on every curve: the end chainage fell past the chords' end and was
    // refused, and the last section of an interval was titled short of it.
    let start = alignment.start_station();
    let end = alignment.end_station();
    let mut chainages = Vec::new();
    if request.stations.is_empty() {
        // cad::section_stations' rule - every interval from the start, and
        // the end - on a straight line as long as the alignment.
        let straight = Polyline2::new(
            vec![Point2::new(0.0, 0.0), Point2::new(alignment.length(), 0.0)],
            false,
        );
        for distance in section_stations(&straight, request.interval)? {
            chainages.push(start + distance);
        }
    } else {
        for &chainage in &request.stations {
            // A station a rounding past an end (as crossSectionLine allows)
            // is the end.
            if !(chainage >= start - tolerance::GEOMETRIC && chainage <= end + tolerance::GEOMETRIC)
            {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "the station is outside the alignment",
                    format!(
                        "CH {} of {} to {}",
                        format_exact_real(chainage),
                        format_exact_real(start),
                        format_exact_real(end)
                    ),
                ));
            }
            chainages.push(cpp::clamp(chainage, start, end));
        }
        chainages.sort_by(f64::total_cmp);
        chainages.dedup();
    }

    let mut cuts = Vec::with_capacity(chainages.len());
    for &chainage in &chainages {
        // Square to the alignment at the chainage, left to right looking
        // along it, as crossSectionLine draws one.
        let left = alignment.point_at_station_offset(chainage, request.half_width);
        let right = alignment.point_at_station_offset(chainage, -request.half_width);
        let (Some(left), Some(right)) = (left, right) else {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "the alignment has no section there",
                format!("CH {}", format_exact_real(chainage)),
            ));
        };
        let across = Polyline2::new(vec![left, right], false);
        let mut cut = Cut {
            chainage,
            low: None,
            high: None,
        };
        if !request.surfaces.is_empty() {
            let options = SectionOptions {
                interval: request.half_width / 50.0,
                include_crossings: false,
                ..SectionOptions::default()
            };
            let section = extract_section(&across, &request.surfaces, None, &options)?;
            let extent = section.extent();
            if !extent.empty() {
                cut.low = Some(extent.min.y);
                cut.high = Some(extent.max.y);
            }
        }
        cuts.push(cut);
    }

    // The cell grid, filled down each column and then across.
    let area = tiling_area(&blank_sheet(&request.paper, String::new()));
    let gutter = TILING_GUTTER_MM;
    let rows = request.rows as f64;
    let columns = request.columns as f64;
    let cell_w = (area.width() - (columns - 1.0) * gutter) / columns;
    let cell_h = (area.height() - (rows - 1.0) * gutter) / rows;
    if !(cell_w > 0.0) || !(cell_h > 0.0) {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "too many rows or columns for the sheet",
        ));
    }
    let mut scale = request.scale;
    if !(scale > 0.0) {
        scale = sheet_scale_at_least(2.0 * request.half_width * 1000.0 / (cell_w * SECTION_FILL))?;
    }
    let mut exaggeration = request.exaggeration;
    if !(exaggeration > 0.0) {
        let mut deepest = 0.0;
        for cut in &cuts {
            if let (Some(low), Some(high)) = (cut.low, cut.high) {
                deepest = cpp::max(deepest, high - low);
            }
        }
        // The painter's ladder (section_fit.rs), so a generated section and
        // one fitted automatically agree. Nothing sampled has no depth to
        // exaggerate: true scale, 1.
        exaggeration = fit_exaggeration(deepest, scale, cell_h * SECTION_FILL, 1.0);
    }

    let per_sheet = request.rows * request.columns;
    let mut sheets: Vec<Sheet> = Vec::new();
    for (i, cut) in cuts.iter().enumerate() {
        let slot = i % per_sheet;
        if slot == 0 {
            let last = (i + per_sheet).min(cuts.len()) - 1;
            sheets.push(blank_sheet(
                &request.paper,
                format!(
                    "{name} CROSS SECTIONS CH {} TO {}",
                    std_fixed(cut.chainage, 3),
                    std_fixed(cuts[last].chainage, 3)
                ),
            ));
        }
        let column = (slot / request.rows) as f64;
        let row = (slot % request.rows) as f64;
        let left = area.min.x + column * (cell_w + gutter);
        let top = area.max.y - row * (cell_h + gutter);
        let mut viewport = Viewport {
            kind: ViewportKind::CrossSections,
            rect: Box2::new(
                Point2::new(left, top - cell_h),
                Point2::new(left + cell_w, top),
            ),
            scale,
            vertical_exaggeration: exaggeration,
            ..Viewport::default()
        };
        viewport.source.alignment = String::from(name);
        viewport.source.stations = vec![cut.chainage];
        viewport.source.section_half_width = request.half_width;
        if let (Some(low), Some(high)) = (cut.low, cut.high) {
            viewport.centre = Point2::new(0.0, (low + high) / 2.0);
        } else {
            viewport.auto_centre = true;
        }
        if let Some(sheet) = sheets.last_mut() {
            sheet.viewports.push(viewport);
        }
    }
    number_sheets(&mut sheets);
    Ok(sheets)
}

/// One sheet per imported plot frame in `model` - a closed polyline carrying
/// plot_frame.width, .height, .scale and the four margins - showing exactly
/// the ground inside the frame's margins, on the ISO paper its size is (within
/// 2 mm), with the frame's own layer hidden in the viewport. Where the paper
/// lies and which way it is turned come from the outline itself (its first
/// corner and first edge), so a frame imported with an origin shift, or moved
/// or rotated since, is plotted where it is drawn; the file's .xorigin,
/// .yorigin and .rotation are not used. Frames that cannot be read - no ISO
/// size, no scale, an outline that is no longer four corners - are skipped and
/// described in `skipped`.
pub fn sheets_from_plot_frames(
    model: &Model,
    paper: &SheetTemplate,
    mut skipped: Option<&mut Vec<String>>,
) -> Result<Vec<Sheet>> {
    // In id order (the ids are listed ascending).
    let frames: Vec<_> = model
        .entities
        .ids()
        .into_iter()
        .filter_map(|id| model.entities.find(id))
        .filter(|entity| entity.properties.contains_key("plot_frame.width"))
        .collect();
    let mut skip = |why: String| {
        if let Some(skipped) = skipped.as_deref_mut() {
            skipped.push(why);
        }
    };
    let mut sheets = Vec::new();
    for frame in frames {
        let p = &frame.properties;
        let label = format!("plot frame {}", frame.id);
        let width = number_property(p, "plot_frame.width");
        let height = number_property(p, "plot_frame.height");
        let scale = number_property(p, "plot_frame.scale").unwrap_or(1000.0);
        let (Some(width), Some(height)) = (width, height) else {
            skip(format!("{label}: no paper size or scale"));
            continue;
        };
        if !(scale > 0.0) {
            skip(format!("{label}: no paper size or scale"));
            continue;
        }
        let Some((iso, landscape)) = iso_paper(width, height) else {
            skip(format!(
                "{label}: {} x {} mm is not an ISO A size",
                format_exact_real(width),
                format_exact_real(height)
            ));
            continue;
        };
        let own = SheetTemplate {
            paper: iso,
            landscape,
            ..paper.clone()
        };
        let mut name = label.clone();
        if let Some(PropertyValue::Text(text)) = frame.metadata.get("12d.name")
            && !text.is_empty()
        {
            name = text.clone();
        }
        let mut sheet = blank_sheet(&own, name);
        let size = paper_dimensions(own.paper, own.landscape);
        let left = number_property(p, "plot_frame.left_margin").unwrap_or(0.0);
        let right = number_property(p, "plot_frame.right_margin").unwrap_or(0.0);
        let top = number_property(p, "plot_frame.top_margin").unwrap_or(0.0);
        let bottom = number_property(p, "plot_frame.bottom_margin").unwrap_or(0.0);
        // The frame's own window on the paper, kept inside this sheet's
        // drawing area so it cannot run over the title block.
        let rect = intersection(
            &Box2::new(
                Point2::new(left, bottom),
                Point2::new(size.width_mm - right, size.height_mm - top),
            ),
            &drawing_area(&sheet),
        );
        if rect.empty() || !(rect.width() > 0.0) || !(rect.height() > 0.0) {
            skip(format!("{label}: its margins leave nothing to draw"));
            continue;
        }
        // Paper millimetres to the ground, from the frame's outline: its
        // first corner is the paper's origin, its first edge runs along the
        // paper's bottom, and its last corner is up the paper's left side
        // (the archive importer draws it so). Not from the plot_frame.xorigin,
        // .yorigin and .rotation properties: they are the file's as it was,
        // while an import with an origin shift, or a later move or rotation
        // of the frame, moves the outline alone - and the sheet would show
        // ground far from the frame.
        let outline = match &frame.geometry {
            Geometry::Polyline(outline) if outline.vertices.len() >= 4 => outline,
            _ => {
                skip(format!(
                    "{label}: its outline is no longer the frame's four corners"
                ));
                continue;
            }
        };
        let origin = outline.vertices[0];
        let first_edge = outline.vertices[1] - origin;
        if !(first_edge.length() > 0.0) {
            skip(format!("{label}: its outline has no direction"));
            continue;
        }
        let rotation = first_edge.angle();
        let along = first_edge.normalized();
        // Up the paper is left of along, unless the frame was mirrored.
        let side = outline.vertices[outline.vertices.len() - 1] - origin;
        let up = if along.cross(side) < 0.0 {
            -along.perpendicular()
        } else {
            along.perpendicular()
        };
        let centre = origin
            + along * metres_across(rect.center().x, scale)
            + up * metres_across(rect.center().y, scale);
        let mut plan = plan_viewport(rect, scale, centre);
        plan.rotation = rotation;
        // The frame is an outline on the drawing; on its own sheet it would
        // be drawn along the paper's edge, over the title block.
        plan.hidden_layers.hide(&frame.layer);
        sheet.viewports.push(plan);
        sheets.push(sheet);
    }
    number_sheets(&mut sheets);
    Ok(sheets)
}

// Puts the extras beside the plan on `sheet`.
fn share_sheet(extras: &mut Vec<Viewport>, sheet: &mut Sheet, preset: TilingPreset) {
    sheet.viewports.append(extras);
    tile_viewports(sheet, preset, TILING_GUTTER_MM);
}

/// Composes sheets for `request` using the presets:
///
/// ```text
/// a plan along an alignment with its long section: plan-and-profile
///     sheets, the plan above (MainBelow) and the long section of the same
///     chainages below at the same horizontal scale;
/// a plan of an area: one sheet at the fitted scale ("auto"), or at a
///     fixed scale, tiles when the area does not fit one sheet at it (those
///     with nothing drawn in them left out when `drawn` is given, and counted
///     in `skipped`);
/// the 3D snapshot and legend beside the plan (MainRight, MainTwoRight)
///     when a plan of an area or one strip along the alignment is the only
///     plan and, made for that narrower cell, still shows all of it - an
///     automatic scale is fitted to the cell, a fixed one must hold it -
///     otherwise on a sheet of their own after the plan;
/// cross sections on sheets of their own after the rest.
/// ```
///
/// "auto" fits everything on as few sheets as it can: an area or an alignment
/// on one sheet. NotFound for an alignment the model does not have;
/// InvalidArgument for a request that shows nothing.
pub fn smart_layout(
    model: &Model,
    request: &LayoutRequest<'_>,
    skipped: Option<&mut Vec<String>>,
) -> Result<Vec<Sheet>> {
    let along_alignment = request.plan_along_alignment
        || request.long_section
        || request.cross_section_interval > 0.0;
    if request.plan_area.is_none() && !along_alignment && !request.model3d && !request.legend {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the layout shows nothing",
        ));
    }
    let solved = if along_alignment {
        Some(solved_alignment(model, &request.alignment)?)
    } else {
        None
    };
    let mut out = Vec::new();
    let area = tiling_area(&blank_sheet(&request.paper, String::new()));
    let plan_along = solved.is_some() && request.plan_along_alignment;

    // The 3D snapshot and the legend.
    let mut extras = Vec::new();
    if request.model3d {
        let mut view = Viewport {
            kind: ViewportKind::Model3D,
            auto_scale: true,
            ..Viewport::default()
        };
        if let Some(plan_area) = &request.plan_area {
            view.centre = plan_area.center();
        } else if let Some(alignment) = &solved {
            view.centre = point_at(
                alignment,
                (alignment.start_station() + alignment.end_station()) / 2.0,
            );
        }
        extras.push(view);
    }
    if request.legend {
        extras.push(Viewport {
            kind: ViewportKind::Legend,
            ..Viewport::default()
        });
    }
    // They share the plan's sheet when the plan is all that sheet would
    // hold - a plan of an area or along the alignment, not both, and no long
    // section - and the plan still shows everything asked of it in the
    // narrower main cell of "Main and panel right" (or "two panels right").
    // The plan is made for that cell from the start, so an automatic scale
    // is fitted to it and a fixed one is tried in it; made for the whole
    // sheet and squeezed afterwards, it lost the ends of a strip or the edges
    // of an area. When it does not fit, it keeps its whole sheet and the
    // extras have a sheet of their own.
    let beside_preset = if extras.len() == 1 {
        TilingPreset::MainRight
    } else {
        TilingPreset::MainTwoRight
    };
    let beside_wanted =
        !extras.is_empty() && !request.long_section && (request.plan_area.is_some() != plan_along);
    let beside_cell = preset_cells(beside_preset, &area, TILING_GUTTER_MM)[0];

    // Strips along the alignment whose plans fill `cell`: at the fixed
    // scale, or at the largest that puts the whole alignment in the cell's
    // width.
    let strips_in = |alignment: &SolvedAlignment, cell: &Box2| -> Result<Vec<Sheet>> {
        let mut scale = request.scale;
        if !(scale > 0.0) {
            scale = sheet_scale_at_least(alignment.length() * 1000.0 / cell.width())?;
        }
        let strip = StripRequest {
            scale,
            paper: request.paper.clone(),
            plan_rect: Some(*cell),
            ..StripRequest::default()
        };
        strip_sheets(alignment, &request.alignment, &strip)
    };

    if request.plan_along_alignment
        && let Some(alignment) = &solved
    {
        let mut strips: Option<Vec<Sheet>> = None;
        // One strip in the narrow cell, or none of this: a failure here only
        // means it does not fit, and the whole-sheet strips below report
        // anything really wrong.
        if beside_wanted
            && let Ok(mut narrow) = strips_in(alignment, &beside_cell)
            && narrow.len() == 1
        {
            share_sheet(&mut extras, &mut narrow[0], beside_preset);
            strips = Some(narrow);
        }
        let strips = match strips {
            Some(strips) => strips,
            None => {
                // Plan and profile: the plan along the alignment above, its
                // long section below at the same horizontal scale - the
                // sheet a road or a pipe is built from.
                let cells = preset_cells(
                    if request.long_section {
                        TilingPreset::MainBelow
                    } else {
                        TilingPreset::Full
                    },
                    &area,
                    TILING_GUTTER_MM,
                );
                let mut whole = strips_in(alignment, &cells[0])?;
                if request.long_section {
                    for sheet in &mut whole {
                        let Some(plan) = sheet.viewports.first() else {
                            continue;
                        };
                        let profile = Viewport {
                            kind: ViewportKind::LongSection,
                            rect: cells[1],
                            scale: plan.scale,
                            vertical_exaggeration: LONG_SECTION_EXAGGERATION,
                            source: plan.source.clone(),
                            centre: Point2::new(
                                (plan.source.chainage_from + plan.source.chainage_to) / 2.0,
                                0.0,
                            ),
                            auto_centre: true,
                            ..Viewport::default()
                        };
                        sheet.viewports.push(profile);
                    }
                }
                whole
            }
        };
        append(&mut out, strips);
    } else if let Some(alignment) = &solved
        && request.long_section
    {
        // Long sections alone: each sheet as many chainages as its width holds.
        let mut scale = request.scale;
        if !(scale > 0.0) {
            scale = sheet_scale_at_least(alignment.length() * 1000.0 / area.width())?;
        }
        let per_sheet = metres_across(area.width(), scale);
        let mut sections = Vec::new();
        let mut at = alignment.start_station();
        while alignment.end_station() - at > 1e-6 {
            let end = cpp::min(at + per_sheet, alignment.end_station());
            let mut sheet = blank_sheet(
                &request.paper,
                format!(
                    "{} LONG SECTION CH {} TO {}",
                    request.alignment,
                    std_fixed(at, 3),
                    std_fixed(end, 3)
                ),
            );
            let mut profile = Viewport {
                kind: ViewportKind::LongSection,
                rect: area,
                scale,
                vertical_exaggeration: LONG_SECTION_EXAGGERATION,
                centre: Point2::new(at + per_sheet / 2.0, 0.0),
                auto_centre: true,
                ..Viewport::default()
            };
            profile.source.alignment = request.alignment.clone();
            profile.source.chainage_from = at;
            profile.source.chainage_to = end;
            sheet.viewports.push(profile);
            sections.push(sheet);
            at += per_sheet;
        }
        number_sheets(&mut sections);
        append(&mut out, sections);
    }

    if let Some(wanted) = request.plan_area {
        // Checked here, not left to fit_to_sheet: at a fixed scale an empty
        // box, whose width and height read as 0, "fits" and would be centred
        // on a NaN, half the sum of its infinite corners.
        if wanted.empty() || !wanted.width().is_finite() || !wanted.height().is_finite() {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "the plan area is empty",
            ));
        }
        // The denominator at which the area just fits `cell`.
        let needed = |cell: &Box2| {
            cpp::max(
                wanted.width() * 1000.0 / cell.width(),
                wanted.height() * 1000.0 / cell.height(),
            )
        };
        let mut plans = Vec::new();
        if beside_wanted {
            // At the fixed scale if the area fits the narrow cell at it; at
            // the largest standard scale that fits it there otherwise
            // ("auto"). An area with nothing to fit - a point, an empty box -
            // is left to fit_to_sheet below to refuse.
            let mut scale = None;
            if request.scale > 0.0 {
                if needed(&beside_cell) <= request.scale {
                    scale = Some(request.scale);
                }
            } else if let Ok(fitted) = sheet_scale_at_least(needed(&beside_cell)) {
                scale = Some(fitted);
            }
            if let Some(scale) = scale {
                let mut sheet = blank_sheet(&request.paper, String::from("PLAN"));
                sheet
                    .viewports
                    .push(plan_viewport(beside_cell, scale, wanted.center()));
                share_sheet(&mut extras, &mut sheet, beside_preset);
                plans.push(sheet);
            }
        }
        if plans.is_empty() && !(request.scale > 0.0) {
            plans = fit_to_sheet(&wanted, &request.paper)?;
        } else if plans.is_empty() {
            if needed(&area) <= request.scale {
                let mut sheet = blank_sheet(&request.paper, String::from("PLAN"));
                sheet
                    .viewports
                    .push(plan_viewport(area, request.scale, wanted.center()));
                plans.push(sheet);
            } else {
                let grid = GridRequest {
                    area: wanted,
                    scale: request.scale,
                    paper: request.paper.clone(),
                    drawn: request.drawn,
                    ..GridRequest::default()
                };
                plans = grid_sheets(&grid, skipped)?;
            }
        }
        append(&mut out, plans);
    }

    // Extras that did not share a plan's sheet have one of their own.
    if !extras.is_empty() {
        let name = if extras.len() == 1 && request.legend {
            "LEGEND"
        } else {
            "3D VIEW"
        };
        let mut sheet = blank_sheet(&request.paper, String::from(name));
        sheet.viewports.append(&mut extras);
        let preset = if sheet.viewports.len() == 1 {
            TilingPreset::Full
        } else {
            TilingPreset::Columns2
        };
        tile_viewports(&mut sheet, preset, TILING_GUTTER_MM);
        append(&mut out, vec![sheet]);
    }

    if let Some(alignment) = &solved
        && request.cross_section_interval > 0.0
    {
        let sections = CrossSectionRequest {
            interval: request.cross_section_interval,
            half_width: request.cross_section_half_width,
            paper: request.paper.clone(),
            ..CrossSectionRequest::default()
        };
        let cut = cross_section_sheets(alignment, &request.alignment, &sections)?;
        append(&mut out, cut);
    }
    Ok(out)
}

/// Gives every sheet and viewport of `sheets` a new id (new_sheet_ids,
/// new_viewport_ids - never one `set` uses or one of its marks still names),
/// and moves the key plans' and match lines' references among `sheets` to
/// their new ids. Generators number from one; this is how their output joins
/// a set that already has sheets.
pub fn prepare_for_append(set: &SheetSet, sheets: &mut [Sheet]) {
    let sheet_ids = new_sheet_ids(set, sheets.len());
    let viewport_count: usize = sheets.iter().map(|sheet| sheet.viewports.len()).sum();
    let viewport_ids = new_viewport_ids(set, viewport_count);
    let mut renamed: BTreeMap<String, String> = BTreeMap::new();
    for (sheet, id) in sheets.iter_mut().zip(&sheet_ids) {
        if !sheet.id.is_empty() {
            renamed
                .entry(std::mem::take(&mut sheet.id))
                .or_insert_with(|| id.clone());
        }
        sheet.id = id.clone();
    }
    let mut next = viewport_ids.into_iter();
    for sheet in sheets.iter_mut() {
        for viewport in &mut sheet.viewports {
            viewport.id = next.next().unwrap_or_default();
            for mark in &mut viewport.marks {
                if let Some(found) = renamed.get(&mark.sheet) {
                    mark.sheet = found.clone();
                }
            }
        }
    }
}
