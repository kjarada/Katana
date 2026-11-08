//! Laying viewports out on a sheet: the tiling presets and snapping
//! (docs/plotting.md; include/katana/cad/plotting/layout.hpp). Pure functions
//! on paper rectangles.
//!
//! The eight presets are the owner's app's, cell for cell, with its 3 mm
//! gutter and its 1 mm inset from the drawing area. What differs is the
//! order viewports take the cells in. The app sorted by a table that had no
//! entry for its map and locality panels, so its comparator returned NaN and a
//! map could land in the big cell or anywhere; here EVERY kind has a rank, and
//! the main drawing - a plan, else a long section, else cross sections - takes
//! the first and largest cell.

use katana_geometry::{Box2, Point2};
use katana_math::cpp;

use crate::plotting::sheet_set::{Sheet, ViewportKind, drawing_area};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TilingPreset {
    /// One cell.
    Full,
    /// Two side by side.
    Columns2,
    /// Two, one above the other.
    Rows2,
    /// Four quarters.
    Quad,
    /// Main 66% wide, a panel to its right.
    MainRight,
    /// Main 64% high, a panel below it.
    MainBelow,
    /// Main 66% high across the top, a panel in the bottom-right corner.
    MainCorner,
    /// Main 64% wide, two panels stacked to its right.
    MainTwoRight,
}

pub const TILING_PRESET_COUNT: usize = 8;
pub const TILING_GUTTER_MM: f64 = 3.0;
pub const TILING_INSET_MM: f64 = 1.0;

// A cell as the owner's app defines it: [x, y, w, h] as fractions of the
// area, measured from its TOP-left - the app draws in SVG, Y down. cell_box
// turns that into paper coordinates, Y up.
#[derive(Clone, Copy)]
struct Fraction {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

const fn fraction(x: f64, y: f64, w: f64, h: f64) -> Fraction {
    Fraction { x, y, w, h }
}

struct PresetDefinition {
    preset: TilingPreset,
    id: &'static str,
    name: &'static str,
    cells: &'static [Fraction],
}

static PRESETS: [PresetDefinition; TILING_PRESET_COUNT] = [
    PresetDefinition {
        preset: TilingPreset::Full,
        id: "full",
        name: "Full frame",
        cells: &[fraction(0.0, 0.0, 1.0, 1.0)],
    },
    PresetDefinition {
        preset: TilingPreset::Columns2,
        id: "cols2",
        name: "Two columns",
        cells: &[fraction(0.0, 0.0, 0.5, 1.0), fraction(0.5, 0.0, 0.5, 1.0)],
    },
    PresetDefinition {
        preset: TilingPreset::Rows2,
        id: "rows2",
        name: "Two rows",
        cells: &[fraction(0.0, 0.0, 1.0, 0.5), fraction(0.0, 0.5, 1.0, 0.5)],
    },
    PresetDefinition {
        preset: TilingPreset::Quad,
        id: "quad",
        name: "Quarters",
        cells: &[
            fraction(0.0, 0.0, 0.5, 0.5),
            fraction(0.5, 0.0, 0.5, 0.5),
            fraction(0.0, 0.5, 0.5, 0.5),
            fraction(0.5, 0.5, 0.5, 0.5),
        ],
    },
    PresetDefinition {
        preset: TilingPreset::MainRight,
        id: "sectionR",
        name: "Main and panel right",
        cells: &[
            fraction(0.0, 0.0, 0.66, 1.0),
            fraction(0.66, 0.0, 0.34, 1.0),
        ],
    },
    PresetDefinition {
        preset: TilingPreset::MainBelow,
        id: "sectionB",
        name: "Main and panel below",
        cells: &[
            fraction(0.0, 0.0, 1.0, 0.64),
            fraction(0.0, 0.64, 1.0, 0.36),
        ],
    },
    PresetDefinition {
        preset: TilingPreset::MainCorner,
        id: "sectionBR",
        name: "Main and panel in the corner",
        cells: &[
            fraction(0.0, 0.0, 1.0, 0.66),
            fraction(0.62, 0.66, 0.38, 0.34),
        ],
    },
    PresetDefinition {
        preset: TilingPreset::MainTwoRight,
        id: "sectionMap3d",
        name: "Main and two panels right",
        cells: &[
            fraction(0.0, 0.0, 0.64, 1.0),
            fraction(0.64, 0.0, 0.36, 0.5),
            fraction(0.64, 0.5, 0.36, 0.5),
        ],
    },
];

fn definition_of(preset: TilingPreset) -> &'static PresetDefinition {
    PRESETS
        .iter()
        .find(|definition| definition.preset == preset)
        .unwrap_or(&PRESETS[0])
}

/// The smallest a tiled cell may make a viewport of this kind (the app's).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SizeMm {
    pub width: f64,
    pub height: f64,
}

// The cell in paper coordinates: its share of the area, less half the gutter
// on every side, at least `minimum` in size, growing right and DOWN from its
// top-left corner as the app's cells do.
fn cell_box(cell: &Fraction, area: &Box2, gutter: f64, minimum: SizeMm) -> Box2 {
    let left = area.min.x + cell.x * area.width() + gutter / 2.0;
    let top = area.max.y - cell.y * area.height() - gutter / 2.0;
    let width = cpp::max(minimum.width, cell.w * area.width() - gutter);
    let height = cpp::max(minimum.height, cell.h * area.height() - gutter);
    Box2::new(
        Point2::new(left, top - height),
        Point2::new(left + width, top),
    )
}

// The smallest shift within `tolerance` that puts one of `mine` on one of
// `targets`, and the target it lands on. Ties keep the first found, so the
// answer does not depend on anything but the order of the inputs.
fn best_shift(mine: &[f64; 3], targets: &[f64], tolerance: f64) -> Option<(f64, f64)> {
    let mut best: Option<(f64, f64)> = None;
    for &own in mine {
        for &target in targets {
            let shift = target - own;
            if shift.abs() <= tolerance && best.is_none_or(|(kept, _)| shift.abs() < kept.abs()) {
                best = Some((shift, target));
            }
        }
    }
    best
}

/// The ids the owner's app gives them: full, cols2, rows2, quad, sectionR,
/// sectionB, sectionBR, sectionMap3d. Stored in files, so never renamed.
pub fn preset_id(preset: TilingPreset) -> &'static str {
    definition_of(preset).id
}

pub fn preset_from_id(id: &str) -> Option<TilingPreset> {
    PRESETS
        .iter()
        .find(|definition| definition.id == id)
        .map(|definition| definition.preset)
}

/// A general name for a menu: "Full frame", "Main and panel below" ...
pub fn preset_name(preset: TilingPreset) -> &'static str {
    definition_of(preset).name
}

/// Which cell a viewport of this kind takes: lower first. Plan 0, LongSection
/// 1, CrossSections 2, Model3D 3, KeyPlan 4, Image 5, Legend 6, Notes 7,
/// SheetIndex 8, Revisions 9.
pub fn tiling_rank(kind: ViewportKind) -> i32 {
    match kind {
        ViewportKind::Plan => 0,
        ViewportKind::LongSection => 1,
        ViewportKind::CrossSections => 2,
        ViewportKind::Model3D => 3,
        ViewportKind::KeyPlan => 4,
        ViewportKind::Image => 5,
        ViewportKind::Legend => 6,
        ViewportKind::Notes => 7,
        // A register sheet's tables: the register takes the big cell, the
        // revisions the panel beside it (register_sheet, tables.rs).
        ViewportKind::SheetIndex => 8,
        ViewportKind::Revisions => 9,
    }
}

/// Notes are placed by hand and tiling leaves them where they are, as the
/// owner's app does; every other kind tiles.
pub fn is_tileable(kind: ViewportKind) -> bool {
    kind != ViewportKind::Notes
}

/// The smallest a tiled cell may make a viewport of this kind (the app's).
pub fn minimum_size(kind: ViewportKind) -> SizeMm {
    // The owner's app's minimum panel sizes, by the panel each kind was
    // (section, map, locality, 3D view, image, note).
    let (width, height) = match kind {
        ViewportKind::LongSection | ViewportKind::CrossSections => (70.0, 45.0),
        ViewportKind::Plan => (35.0, 30.0),
        ViewportKind::KeyPlan => (30.0, 26.0),
        ViewportKind::Model3D => (30.0, 24.0),
        ViewportKind::Image => (8.0, 8.0),
        ViewportKind::Legend | ViewportKind::Notes => (16.0, 8.0),
        // Kinds the app did not have: room for the columns and a few rows of
        // the smallest text a table is set in (tables.rs).
        ViewportKind::SheetIndex => (70.0, 30.0),
        ViewportKind::Revisions => (50.0, 20.0),
    };
    SizeMm { width, height }
}

/// The preset's cells inside `area`, first cell first: each the cell's share
/// of the area less half the gutter on every side. `area` is where tiling
/// happens - the drawing area already inset (tiling_area). The C++'s default
/// gutter is TILING_GUTTER_MM.
pub fn preset_cells(preset: TilingPreset, area: &Box2, gutter_mm: f64) -> Vec<Box2> {
    definition_of(preset)
        .cells
        .iter()
        .map(|cell| cell_box(cell, area, gutter_mm, SizeMm::default()))
        .collect()
}

/// The drawing area inset by TILING_INSET_MM: what the presets divide.
pub fn tiling_area(sheet: &Sheet) -> Box2 {
    drawing_area(sheet).inflated(-TILING_INSET_MM)
}

/// Lays the sheet's tileable, unlocked viewports into the preset's cells in
/// rank order (ties keep their order on the sheet), never smaller than the
/// kind's minimum. Viewports beyond the cell count stay where they were.
/// Returns how many were placed.
pub fn tile_viewports(sheet: &mut Sheet, preset: TilingPreset, gutter_mm: f64) -> usize {
    let mut order: Vec<usize> = (0..sheet.viewports.len())
        .filter(|&index| {
            let viewport = &sheet.viewports[index];
            !viewport.locked && is_tileable(viewport.kind)
        })
        .collect();
    // A stable sort, as the C++'s std::stable_sort.
    order.sort_by_key(|&index| tiling_rank(sheet.viewports[index].kind));
    let area = tiling_area(sheet);
    let cells = definition_of(preset).cells;
    let placed = order.len().min(cells.len());
    for (cell, &index) in cells.iter().zip(order.iter()).take(placed) {
        let minimum = minimum_size(sheet.viewports[index].kind);
        sheet.viewports[index].rect = cell_box(cell, &area, gutter_mm, minimum);
    }
    placed
}

/// A moving rectangle snapped to the drawing area and to other viewports:
/// the smallest move, within `tolerance_mm` on each axis, that puts one of its
/// edges or its centre line on an edge or centre line of a target. `guide_x`
/// and `guide_y` are the lines it snapped to, for the editor to draw.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SnapResult {
    pub rect: Box2,
    pub guide_x: Option<f64>,
    pub guide_y: Option<f64>,
}

pub fn snap_rect(
    moving: &Box2,
    drawing_area: &Box2,
    others: &[Box2],
    tolerance_mm: f64,
) -> SnapResult {
    let mut result = SnapResult {
        rect: *moving,
        guide_x: None,
        guide_y: None,
    };
    if !(tolerance_mm > 0.0) || moving.empty() {
        return result;
    }
    let mut xs = Vec::new();
    let mut ys = Vec::new();
    let mut add_targets = |target: &Box2| {
        if target.empty() {
            return;
        }
        xs.extend([target.min.x, target.max.x, target.center().x]);
        ys.extend([target.min.y, target.max.y, target.center().y]);
    };
    add_targets(drawing_area);
    for other in others {
        add_targets(other);
    }
    let own_x = [moving.min.x, moving.max.x, moving.center().x];
    let own_y = [moving.min.y, moving.max.y, moving.center().y];
    if let Some((shift, target)) = best_shift(&own_x, &xs, tolerance_mm) {
        result.rect.min.x += shift;
        result.rect.max.x += shift;
        result.guide_x = Some(target);
    }
    if let Some((shift, target)) = best_shift(&own_y, &ys, tolerance_mm) {
        result.rect.min.y += shift;
        result.rect.max.y += shift;
        result.guide_y = Some(target);
    }
    result
}
