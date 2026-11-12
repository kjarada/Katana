//! Sections on paper (docs/plotting.md, "Section smarts"): a LongSection or CrossSections viewport
//! drawn, and its automatic scale chosen. The port of plotting/section_painter.
//!
//! What a section decides with numbers - its automatic scale and exaggeration, its label steps,
//! its layout, cut and fill, each crossing's note and where every label goes - is decided headless
//! in cad's plotting::section_fit and section_annotation; this measures the text and draws what
//! they decide:
//!
//! ```text
//! the plot     cut and fill shaded between the design and the ground (light red where the
//!              ground stands above the design, light green where it is below), split where the
//!              two cross; the grid at steps chosen from the measured labels; the series; a
//!              chainage range's ends; all clipped to the plot.
//! inside it    the datum, the surface key, a cross section's design and ground levels (and the
//!              cut or fill) at its centreline, and each crossing's note ("WATER RL 24.10 D
//!              1.20") stood up beside its line with its offset under the marker - placed clear
//!              of each other, the nearest the middle first, and dropped (counted) rather than
//!              drawn over another.
//! outside it   the levels up the left, the axis values or a long section's data band (design,
//!              ground, CUT/FILL, chainage), a cross section's "CH" caption - none outside the
//!              viewport.
//! ```
//!
//! The sheet painter hands its paper mapping, pens and text setter in through SectionCanvas, so
//! every colour and line weight still passes through the one place that styles them.

#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "The C++'s !(a > b) is kept: it rejects NaN, which a > b alone would let through."
)]

use std::sync::Arc;

use katana_cad::layer_overrides::LayerOverrides;
use katana_cad::plotting::section_annotation::{
    Earthwork, LabelCandidate, cut_fill_at, cut_fill_text, design_series, earthwork_regions,
    ground_series, is_design_series, level_at, level_range, place_labels,
};
use katana_cad::plotting::section_fit::{
    SECTION_BAND_ROW_MM, SectionFit, SectionFitRequest, SectionLayoutRequest, fit_section,
    grid_values, label_step, section_drawing_area, section_plot_layout, section_span, step_text,
    viewport_half_width, viewport_stations,
};
use katana_cad::plotting::{HorizontalJustify, VerticalJustify, Viewport, ViewportKind};
use katana_cad::section::{Section, SectionSurface};
use katana_core::text::{format_exact_real, format_fixed};
use katana_entity::Model;
use katana_geometry::{Box2, Point2};

use crate::canvas::{Brush, Canvas, Color, FillRule, Pen, PointF, RectF};

/// A label's look: the sheet painter's text style, less its colour (a section's words are all in
/// the ink).
#[derive(Clone, Debug, PartialEq)]
pub struct SectionTextStyle {
    pub cap_mm: f64,
    pub x_factor: f64,
    pub horizontal: HorizontalJustify,
    pub vertical: VerticalJustify,
    pub angle_degrees: f64,
    /// 0: 1.5 x the cap height.
    pub line_spacing_mm: f64,
    pub bold: bool,
}

impl Default for SectionTextStyle {
    fn default() -> Self {
        Self {
            cap_mm: 1.4,
            x_factor: 0.9,
            horizontal: HorizontalJustify::Left,
            vertical: VerticalJustify::Bottom,
            angle_degrees: 0.0,
            line_spacing_mm: 0.0,
            bold: false,
        }
    }
}

/// What a section is drawn with: the sheet painter's paper (millimetres from the bottom-left, Y
/// up, onto the device), its pens and fills, and its text setter. SH1's sheet painter implements
/// it over its paper pen, dashed pen and text setter.
pub trait SectionCanvas {
    fn painter(&mut self) -> &mut dyn Canvas;
    /// A point of the paper on the device.
    fn at(&self, paper: Point2) -> PointF;
    /// A box of the paper on the device (the C++'s at(Box2)).
    fn at_box(&self, paper: &Box2) -> RectF;
    /// Paper millimetres in device units.
    fn mm(&self, millimetres: f64) -> f64;
    fn pen(&self, colour: Color, width_mm: f64) -> Pen;
    fn dashed_pen(&self, colour: Color, width_mm: f64, pattern_mm: &[f64]) -> Pen;
    fn fill(&self, colour: Color) -> Brush;
    /// A line's width in paper millimetres, set in `style`. Measuring needs only the fonts, so it
    /// reads the canvas (the C++'s textWidthMm was not const; label_step asks it from a Fn).
    fn text_width_mm(&self, line: &str, style: &SectionTextStyle) -> f64;
    fn text(&mut self, anchor: Point2, text: &str, style: &SectionTextStyle, squeeze: f64);
    /// A white box behind a label drawn over the drawing.
    fn knock_out(&mut self, paper: &Box2);
}

/// The long section along the viewport's alignment and the chainage its station 0 is at; the
/// failure said when there is none. The sections are the sheet painter's cache's, shared.
pub type LongSectionCut<'a> = Box<dyn FnMut() -> Result<(Arc<Section>, f64), String> + 'a>;
/// The cross section at a chainage, a half width each side; the failure said when there is none.
pub type CrossSectionCut<'a> = Box<dyn FnMut(f64, f64) -> Result<Arc<Section>, String> + 'a>;
/// The alignment's design profile's level at a chainage; nothing without a profile.
pub type ProfileLevel<'a> = Box<dyn Fn(f64) -> Option<f64> + 'a>;

/// The cut sections a viewport draws, from the sheet painter's cache.
pub struct SectionCuts<'a> {
    pub long_section: LongSectionCut<'a>,
    pub cross_section: CrossSectionCut<'a>,
    pub profile_level: Option<ProfileLevel<'a>>,
}

/// How a section viewport's drawing went.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SectionPaintResult {
    pub drawn: bool,
    /// "no alignment chosen", a cut's failure.
    pub problems: Vec<String>,
    /// The sheet painter's stats' section notes dropped.
    pub notes_dropped: usize,
}

// The sheet painter's inks: the frame's black, the grid's light grey, a faint grey for
// construction.
const INK: Color = Color::rgb(0, 0, 0);
const GRID_INK: Color = Color::rgb(190, 190, 190);
const FAINT: Color = Color::rgb(150, 150, 150);

const GROUND_INKS: [Color; 5] = [
    Color::rgb(125, 80, 40),
    Color::rgb(0, 125, 50),
    Color::rgb(0, 90, 200),
    Color::rgb(150, 0, 150),
    Color::rgb(210, 120, 0),
];
const DESIGN_INK: Color = Color::rgb(210, 0, 0);
// Cut and fill, shaded between the design and the ground: light red where the ground stands above
// the design and is dug away, light green where the design stands above the ground and is built
// up.
const CUT_SHADE: Color = Color::rgb(250, 200, 200);
const FILL_SHADE: Color = Color::rgb(200, 236, 200);

/// A long section with no levels to fit is drawn as long sections usually are: H 1:500 V 1:50.
const LONG_SECTION_EXAGGERATION: f64 = 10.0;

/// `value` as a label on a grid of `step` (plotting's step_text).
fn number(value: f64, step: f64) -> String {
    step_text(value, step)
}

/// The pen of each series, in the order they were cut: the design red and heavier, each ground a
/// colour of its own.
fn series_pens(canvas: &dyn SectionCanvas, section: &Section) -> Vec<Pen> {
    let mut pens = Vec::new();
    let mut ground = 0;
    for surface in &section.surfaces {
        let design = is_design_series(&surface.name);
        pens.push(canvas.pen(
            if design {
                DESIGN_INK
            } else {
                GROUND_INKS[ground % GROUND_INKS.len()]
            },
            if design { 0.5 } else { 0.35 },
        ));
        ground += usize::from(!design);
    }
    pens
}

/// A row of a long section's data band: a series' levels, the cut or fill between the design and
/// the ground, or - neither - the chainage.
#[derive(Clone, Copy)]
struct BandRow<'s> {
    series: Option<&'s SectionSurface>,
    cut_fill: bool,
}

/// The band's rows, top down: the design first - the level a long section is read for - then the
/// ground, two series at most; the cut or fill when there are both; the chainage last.
fn band_rows(section: &Section) -> Vec<BandRow<'_>> {
    let mut rows = Vec::new();
    for design in [true, false] {
        for surface in &section.surfaces {
            if is_design_series(&surface.name) == design {
                rows.push(BandRow {
                    series: Some(surface),
                    cut_fill: false,
                });
            }
        }
    }
    rows.truncate(2);
    if design_series(section).is_some() && ground_series(section).is_some() {
        rows.push(BandRow {
            series: None,
            cut_fill: true,
        });
    }
    rows.push(BandRow {
        series: None,
        cut_fill: false,
    });
    rows
}

/// The level labels up a section's left side.
fn level_label_style() -> SectionTextStyle {
    SectionTextStyle {
        cap_mm: 1.5,
        horizontal: HorizontalJustify::Right,
        vertical: VerticalJustify::Middle,
        ..SectionTextStyle::default()
    }
}

/// A line of text drawn in the box place_labels gave it: `turned` stands it up from the box's
/// foot, reading upwards; otherwise it sits on the box's base.
fn draw_in_box(
    canvas: &mut dyn SectionCanvas,
    place: &Box2,
    line: &str,
    style: &SectionTextStyle,
    turned: bool,
) {
    let mut style = style.clone();
    style.horizontal = HorizontalJustify::Left;
    if turned {
        style.angle_degrees = 90.0;
        style.vertical = VerticalJustify::Middle;
        canvas.text(
            Point2::new(place.center().x, place.min.y),
            line,
            &style,
            1.0,
        );
    } else {
        style.angle_degrees = 0.0;
        style.vertical = VerticalJustify::Bottom;
        canvas.text(place.min, line, &style, 1.0);
    }
}

/// `levels` widened to take in `level` too.
fn widened(levels: Option<(f64, f64)>, level: Option<f64>) -> Option<(f64, f64)> {
    let Some(level) = level else {
        return levels;
    };
    match levels {
        None => Some((level, level)),
        Some((low, high)) => Some((
            katana_math::cpp::min(low, level),
            katana_math::cpp::max(high, level),
        )),
    }
}

/// How one section is drawn in its box.
#[derive(Clone)]
struct SectionAxes<'a> {
    /// 1 : scale horizontally.
    scale: f64,
    /// The vertical scale is scale / exaggeration.
    exaggeration: f64,
    /// (axis value, elevation) at the plot's middle.
    centre: Point2,
    auto_centre: bool,
    /// Axis value = station + shift.
    shift: f64,
    /// A long section's chainages: exactly these are shown, ends labelled.
    range: Option<(f64, f64)>,
    /// A cross section: the centreline at 0.
    cross: bool,
    /// A long section's levels under it.
    data_band: bool,
    hidden: Option<&'a LayerOverrides>,
    /// Under a cross section: its chainage.
    caption: String,
    /// Gives the crossings their own levels (their entities' heights).
    model: Option<&'a Model>,
    /// A cross section's design level at its centreline from the alignment's profile: read there
    /// when no design series gives one.
    profile_level: Option<f64>,
}

impl Default for SectionAxes<'_> {
    fn default() -> Self {
        Self {
            scale: 500.0,
            exaggeration: 1.0,
            centre: Point2::new(0.0, 0.0),
            auto_centre: false,
            shift: 0.0,
            range: None,
            cross: false,
            data_band: false,
            hidden: None,
            caption: String::new(),
            model: None,
            profile_level: None,
        }
    }
}

/// How a placed label is drawn in the place it gets; the place index given is into the
/// candidate's boxes followed by its fallback's.
type DrawPlaced<'a> = Box<dyn Fn(&mut dyn SectionCanvas, usize) + 'a>;

/// A label placed with the others (plotting's place_labels), and how to draw it in the place it
/// gets.
struct Placed<'a> {
    candidate: LabelCandidate,
    draw: DrawPlaced<'a>,
    /// Counted when dropped.
    note: bool,
    /// Places tried only once every label has had its own: a last resort that must not take a
    /// place another label could have had.
    fallback: Vec<Box2>,
}

impl<'a> Placed<'a> {
    fn new(boxes: Vec<Box2>, priority: f64, draw: DrawPlaced<'a>) -> Self {
        Self {
            candidate: LabelCandidate { boxes, priority },
            draw,
            note: false,
            fallback: Vec::new(),
        }
    }
}

/// Places `labels` in `bounds` clear of `obstacles` and draws each that has a place; returns how
/// many notes had none.
fn place_and_draw(
    canvas: &mut dyn SectionCanvas,
    labels: &[Placed],
    bounds: &Box2,
    obstacles: &[Box2],
    gap_mm: f64,
) -> usize {
    let candidates: Vec<LabelCandidate> =
        labels.iter().map(|label| label.candidate.clone()).collect();
    let mut places = place_labels(&candidates, bounds, obstacles, gap_mm);
    // The last resorts, in the same order, around everything placed.
    let mut taken: Vec<Box2> = obstacles.to_vec();
    for (i, place) in places.iter().enumerate() {
        if let Some(place) = place {
            taken.push(candidates[i].boxes[*place]);
        }
    }
    let mut waiting = Vec::new();
    let mut last_resorts = Vec::new();
    for (i, label) in labels.iter().enumerate() {
        if places[i].is_none() && !label.fallback.is_empty() {
            waiting.push(i);
            last_resorts.push(LabelCandidate {
                boxes: label.fallback.clone(),
                priority: candidates[i].priority,
            });
        }
    }
    let resorted = place_labels(&last_resorts, bounds, &taken, gap_mm);
    for (w, &index) in waiting.iter().enumerate() {
        if let Some(resort) = resorted[w] {
            places[index] = Some(candidates[index].boxes.len() + resort);
        }
    }

    let mut dropped = 0;
    for (label, place) in labels.iter().zip(&places) {
        if let Some(place) = place {
            (label.draw)(&mut *canvas, *place);
        } else if label.note {
            dropped += 1;
        }
    }
    dropped
}

fn set_pen(canvas: &mut dyn SectionCanvas, pen: Pen) {
    canvas.painter().set_pen(pen);
}

/// A line between two points of the paper, in the canvas's pen.
fn line(canvas: &mut dyn SectionCanvas, from: Point2, to: Point2) {
    let (a, b) = (canvas.at(from), canvas.at(to));
    canvas.painter().draw_line(a, b);
}

/// A crossing as it is drawn: where, its marker when inside the plot, and its note.
struct Crossing {
    x: f64,
    /// The marker, when it is inside the plot.
    y: Option<f64>,
    /// Chainage or offset.
    along: f64,
    note: katana_cad::plotting::CrossingNote,
}

/// Draws `section` in `area`; returns how many of its notes it had no room for (the sheet's
/// section notes dropped), or - nothing drawn - why not.
fn paint_section(
    canvas: &mut dyn SectionCanvas,
    area: &Box2,
    section: &Section,
    axes: &SectionAxes,
) -> Result<usize, String> {
    let design = design_series(section);
    let ground = ground_series(section);
    let rows = if axes.data_band {
        band_rows(section)
    } else {
        Vec::new()
    };
    // The stations shown: a long section's range and nothing past it, else all of it.
    let from_station = axes
        .range
        .map_or(f64::NEG_INFINITY, |range| range.0 - axes.shift);
    let to_station = axes
        .range
        .map_or(f64::INFINITY, |range| range.1 - axes.shift);
    let levels = widened(
        level_range(section, from_station, to_station, axes.model, axes.hidden),
        axes.profile_level,
    );

    let h_mm = 1000.0 / axes.scale;
    let v_mm = h_mm * axes.exaggeration;
    let mut centre = axes.centre;
    if axes.auto_centre {
        if let Some((low, high)) = levels {
            centre.y = 0.5 * (low + high);
        }
        if let Some((first, last)) = axes.range {
            centre.x = 0.5 * (first + last);
        } else if axes.cross {
            centre.x = 0.0;
        } else {
            let extent = section.extent();
            if !extent.empty() {
                centre.x = extent.center().x + axes.shift;
            }
        }
    }

    // The plot, laid out around the widest level label: the levels' step depends only on the
    // plot's height, so it is known before the width is.
    let level_style = level_label_style();
    let mut request = SectionLayoutRequest {
        area: *area,
        band_rows: rows.len(),
        caption: !axes.caption.is_empty(),
        ..SectionLayoutRequest::default()
    };
    let no_room = || {
        format!(
            "no room for a section in {} x {} mm",
            format_fixed(area.width(), 1),
            format_fixed(area.height(), 1)
        )
    };
    let mut layout = section_plot_layout(&request);
    if layout.plot.empty() {
        return Err(no_room());
    }
    let mid_y = layout.plot.center().y;
    let low_level = centre.y - 0.5 * layout.plot.height() / v_mm;
    let high_level = centre.y + 0.5 * layout.plot.height() / v_mm;
    // Level labels are stacked up the side: their height sets their step, 7 mm apart at the
    // least so the grid stays open. Never finer than the millimetre their three decimals can
    // write.
    let level_cap = |_: f64| level_style.cap_mm;
    let v_step = katana_math::cpp::max(label_step(v_mm, 7.0, 1.0, Some(&level_cap)), 0.001);
    let level_values = grid_values(low_level, high_level, v_step, 200);
    request.level_label_mm = 0.0;
    for &level in &level_values {
        request.level_label_mm = katana_math::cpp::max(
            request.level_label_mm,
            canvas.text_width_mm(&number(level, v_step), &level_style),
        );
    }
    layout = section_plot_layout(&request);
    if layout.plot.empty() {
        return Err(no_room());
    }
    let mut plot = layout.plot;
    let banded = layout.banded;
    let mid_x = plot.center().x;
    let x_of = |value: f64| mid_x + (value - centre.x) * h_mm;
    let y_of = |level: f64| mid_y + (level - centre.y) * v_mm;
    // A chainage range is shown exactly: the plot ends where it does.
    if let Some((first, last)) = axes.range {
        plot.min.x = katana_math::cpp::max(plot.min.x, x_of(first));
        plot.max.x = katana_math::cpp::min(plot.max.x, x_of(last));
        if !(plot.width() > 1.0) {
            return Err(format!(
                "chainages {} to {} are off the plot centred on {}",
                format_fixed(first, 3),
                format_fixed(last, 3),
                format_fixed(centre.x, 3)
            ));
        }
    }
    let first_value = centre.x + (plot.min.x - mid_x) / h_mm;
    let last_value = centre.x + (plot.max.x - mid_x) / h_mm;

    let small = SectionTextStyle::default();
    let values = SectionTextStyle {
        cap_mm: 1.5,
        ..small.clone()
    };
    let cell = small.clone(); // a band value, turned across its row

    // The step along: a band's values are turned across it, so they need only their height; the
    // axis values need their width.
    let h_step = if banded {
        let cell_cap = |_: f64| cell.cap_mm;
        label_step(h_mm, 10.0, 1.0, Some(&cell_cap))
    } else {
        let widest = |step: f64| {
            let mut widest: f64 = 0.0;
            for v in grid_values(first_value, last_value, step, 200) {
                widest =
                    katana_math::cpp::max(widest, canvas.text_width_mm(&number(v, step), &values));
            }
            widest
        };
        label_step(h_mm, 10.0, 2.0, Some(&widest))
    };
    let along_values = grid_values(first_value, last_value, h_step, 400);
    let pens = series_pens(canvas, section);

    // ---- under the plot's clip: shading, grid, lines ----
    let plot_device = canvas.at_box(&plot);
    canvas.painter().save();
    canvas.painter().set_clip_rect(plot_device, true);

    // Cut and fill, under everything else.
    if let (Some(design), Some(ground)) = (design, ground) {
        canvas.painter().set_pen(Pen::none());
        for region in earthwork_regions(design, ground, from_station, to_station) {
            let polygon: Vec<PointF> = region
                .outline
                .iter()
                .map(|p| canvas.at(Point2::new(x_of(p.x + axes.shift), y_of(p.y))))
                .collect();
            let shade = canvas.fill(if region.kind == Earthwork::Cut {
                CUT_SHADE
            } else {
                FILL_SHADE
            });
            canvas.painter().set_brush(shade);
            canvas.painter().draw_polygon(&polygon, FillRule::OddEven);
        }
        canvas.painter().set_brush(Brush::none());
    }

    let grid_pen = canvas.pen(GRID_INK, 0.13);
    set_pen(canvas, grid_pen);
    for &v in &along_values {
        line(
            canvas,
            Point2::new(x_of(v), plot.min.y),
            Point2::new(x_of(v), plot.max.y),
        );
    }
    for &e in &level_values {
        line(
            canvas,
            Point2::new(plot.min.x, y_of(e)),
            Point2::new(plot.max.x, y_of(e)),
        );
    }

    // The centreline of a cross section.
    let centreline = axes.cross && x_of(0.0) > plot.min.x && x_of(0.0) < plot.max.x;
    if centreline {
        let dashed = canvas.dashed_pen(INK, 0.25, &[6.0, 1.2, 0.6, 1.2]);
        set_pen(canvas, dashed);
        line(
            canvas,
            Point2::new(x_of(0.0), plot.min.y),
            Point2::new(x_of(0.0), plot.max.y),
        );
    }

    // Crossings: where the drawing's lines cut the section, marked at their level when they have
    // one; their notes are placed below with the rest.
    let mut crossings: Vec<Crossing> = Vec::new();
    for crossing in &section.crossings {
        if axes
            .hidden
            .is_some_and(|hidden| hidden.hides(&crossing.layer))
        {
            continue;
        }
        let along = crossing.station + axes.shift;
        let x = x_of(along);
        if x <= plot.min.x || x >= plot.max.x {
            continue;
        }
        let note =
            katana_cad::plotting::section_annotation::crossing_note(section, crossing, axes.model);
        let dashed = canvas.dashed_pen(FAINT, 0.18, &[1.5, 1.0]);
        set_pen(canvas, dashed);
        line(
            canvas,
            Point2::new(x, plot.min.y),
            Point2::new(x, plot.max.y),
        );
        crossings.push(Crossing {
            x,
            y: None,
            along,
            note,
        });
    }

    // Surfaces, broken at every gap.
    for (s, surface) in section.surfaces.iter().enumerate() {
        set_pen(canvas, pens[s].clone());
        let mut run: Vec<PointF> = Vec::new();
        for sample in &surface.samples {
            let Some(elevation) = sample.elevation else {
                if run.len() > 1 {
                    canvas.painter().draw_polyline(&run);
                }
                run.clear();
                continue;
            };
            run.push(canvas.at(Point2::new(
                x_of(sample.station + axes.shift),
                y_of(elevation),
            )));
        }
        if run.len() > 1 {
            canvas.painter().draw_polyline(&run);
        }
    }

    // The crossings' markers, over the series they sit on or under.
    for at in &mut crossings {
        let Some(level) = at.note.level else {
            continue;
        };
        let y = y_of(level);
        let marker_pen = canvas.pen(INK, 0.25);
        let radius = canvas.mm(0.8);
        let centre_device = canvas.at(Point2::new(at.x, y));
        let painter = canvas.painter();
        painter.set_pen(marker_pen);
        painter.set_brush(Brush::solid(Color::WHITE));
        painter.draw_ellipse(RectF::new(
            centre_device.x - radius,
            centre_device.y - radius,
            2.0 * radius,
            2.0 * radius,
        ));
        painter.set_brush(Brush::none());
        if y > plot.min.y && y < plot.max.y {
            at.y = Some(y);
        }
    }
    canvas.painter().restore();

    // The plot's frame; a range's ends heavier, as where the section stops.
    let frame_pen = canvas.pen(INK, 0.25);
    set_pen(canvas, frame_pen);
    canvas.painter().set_brush(Brush::none());
    canvas.painter().draw_rect(plot_device);
    // (chainage, paper x) of the range's ends in view.
    let mut ends: Vec<(f64, f64)> = Vec::new();
    if let Some((first, last)) = axes.range {
        let end_pen = canvas.pen(INK, 0.35);
        set_pen(canvas, end_pen);
        for end in [first, last] {
            let x = x_of(end);
            if x >= plot.min.x - 1e-6 && x <= plot.max.x + 1e-6 {
                line(
                    canvas,
                    Point2::new(x, plot.min.y),
                    Point2::new(x, plot.max.y),
                );
                ends.push((end, x));
            }
        }
    }

    // ---- labels inside the plot ----
    // The datum and the surface key, the centreline's levels, and each crossing's note and offset:
    // placed so that none covers another, the nearest the middle kept first.
    let mut inside: Vec<Placed> = Vec::new();
    // A note is never written over another crossing's line or the centreline, where it would seem
    // to name that one: such places are not offered. Its own line is the one at `own`.
    let mut lines_at: Vec<f64> = crossings.iter().map(|at| at.x).collect();
    if centreline {
        lines_at.push(x_of(0.0));
    }
    let on_another_line = |place: &Box2, own: f64| {
        const CLEAR: f64 = 0.25;
        lines_at
            .iter()
            .any(|&x| x != own && place.min.x < x + CLEAR && place.max.x > x - CLEAR)
    };

    {
        let datum = SectionTextStyle {
            cap_mm: 1.5,
            ..small.clone()
        };
        let datum_text = format!("DATUM RL {}", format_fixed(low_level, 2));
        let w = canvas.text_width_mm(&datum_text, &datum);
        let place = Box2::new(
            Point2::new(plot.min.x + 1.0, plot.min.y + 1.0),
            Point2::new(plot.min.x + 1.0 + w, plot.min.y + 1.0 + datum.cap_mm),
        );
        inside.push(Placed::new(
            vec![place],
            -3.0,
            Box::new(move |canvas: &mut dyn SectionCanvas, _: usize| {
                canvas.knock_out(&place.inflated(0.4));
                draw_in_box(canvas, &place, &datum_text, &datum, false);
            }),
        ));
    }

    // The surface key, top left.
    if !section.surfaces.is_empty() {
        let key = SectionTextStyle {
            cap_mm: 1.5,
            ..small.clone()
        };
        let mut widest: f64 = 0.0;
        for surface in &section.surfaces {
            widest = katana_math::cpp::max(
                widest,
                canvas.text_width_mm(&surface.name.to_uppercase(), &key),
            );
        }
        let pitch = 3.2;
        let count = section.surfaces.len() as f64;
        let place = Box2::new(
            Point2::new(
                plot.min.x + 0.6,
                plot.max.y - 2.5 - pitch * (count - 1.0) - 1.8,
            ),
            Point2::new(plot.min.x + 9.5 + widest, plot.max.y - 0.6),
        );
        let pens = &pens;
        inside.push(Placed::new(
            vec![place],
            -2.0,
            Box::new(move |canvas: &mut dyn SectionCanvas, _: usize| {
                canvas.knock_out(&place);
                let row = SectionTextStyle {
                    vertical: VerticalJustify::Middle,
                    ..key.clone()
                };
                let mut y = plot.max.y - 2.5;
                for (s, surface) in section.surfaces.iter().enumerate() {
                    set_pen(canvas, pens[s].clone());
                    line(
                        canvas,
                        Point2::new(plot.min.x + 1.5, y),
                        Point2::new(plot.min.x + 7.0, y),
                    );
                    canvas.text(
                        Point2::new(plot.min.x + 8.0, y),
                        &surface.name.to_uppercase(),
                        &row,
                        1.0,
                    );
                    y -= pitch;
                }
            }),
        ));
    }

    // A cross section's design and ground levels at the centreline, and the cut or fill between
    // them. The design is the design series', else the alignment's profile's.
    if centreline {
        let station = -axes.shift;
        let mut design_level = design.and_then(|series| level_at(series, station));
        let design_series_level = design_level.is_some();
        if design_level.is_none() {
            design_level = axes.profile_level;
        }
        let ground_level = ground.and_then(|series| level_at(series, station));
        let series_pen = |series: &SectionSurface| {
            section
                .surfaces
                .iter()
                .position(|surface| std::ptr::eq(surface, series))
                .map(|index| pens[index].clone())
                .unwrap_or_default()
        };
        let mut lines: Vec<String> = Vec::new();
        if let Some(level) = design_level {
            let name = match design {
                Some(series) if design_series_level => series.name.to_uppercase(),
                _ => String::from("DESIGN"),
            };
            lines.push(format!("{name} RL {}", format_fixed(level, 3)));
        }
        if let (Some(level), Some(series)) = (ground_level, ground) {
            lines.push(format!(
                "{} RL {}",
                series.name.to_uppercase(),
                format_fixed(level, 3)
            ));
        }
        if let (Some(design_at), Some(ground_at)) = (design_level, ground_level)
            && (design_at - ground_at).abs() >= 0.0005
        {
            // CUT where the ground is above the design, FILL where below.
            let design_less_ground = design_at - ground_at;
            lines.push(format!(
                "{} {}",
                if design_less_ground > 0.0 {
                    "FILL"
                } else {
                    "CUT"
                },
                format_fixed(design_less_ground.abs(), 3)
            ));
        }
        if !lines.is_empty() {
            // A tick across the centreline at each level read, in its series' pen.
            let x0 = x_of(0.0);
            let mut ticks: Vec<(Pen, f64)> = Vec::new();
            if let Some(level) = design_level {
                let tick_pen = match design {
                    Some(series) if design_series_level => series_pen(series),
                    _ => canvas.pen(DESIGN_INK, 0.5),
                };
                ticks.push((tick_pen, level));
            }
            if let (Some(series), Some(level)) = (ground, ground_level) {
                ticks.push((series_pen(series), level));
            }
            for (tick_pen, level) in ticks {
                if !(y_of(level) > plot.min.y) || !(y_of(level) < plot.max.y) {
                    continue;
                }
                set_pen(canvas, tick_pen);
                line(
                    canvas,
                    Point2::new(x0 - 1.0, y_of(level)),
                    Point2::new(x0 + 1.0, y_of(level)),
                );
            }
            let note = SectionTextStyle {
                line_spacing_mm: 2.3,
                ..small.clone()
            };
            let mut w: f64 = 0.0;
            for text in &lines {
                w = katana_math::cpp::max(w, canvas.text_width_mm(text, &note));
            }
            let h = note.line_spacing_mm * (lines.len() - 1) as f64 + note.cap_mm;
            // Beside the centreline just above the higher level, else at the plot's top, else at
            // its foot; right of it first.
            let mut above = plot.min.y + 1.0;
            for level in [design_level, ground_level].into_iter().flatten() {
                above = katana_math::cpp::max(above, y_of(level) + 1.2);
            }
            let mut clear: Vec<Box2> = Vec::new();
            let mut across: Vec<Box2> = Vec::new();
            for base in [above, plot.max.y - 1.0 - h, plot.min.y + 1.0] {
                for place in [
                    Box2::new(
                        Point2::new(x0 + 0.8, base),
                        Point2::new(x0 + 0.8 + w, base + h),
                    ),
                    Box2::new(
                        Point2::new(x0 - 0.8 - w, base),
                        Point2::new(x0 - 0.8, base + h),
                    ),
                ] {
                    if on_another_line(&place, x0) {
                        across.push(place);
                    } else {
                        clear.push(place);
                    }
                }
            }
            // Clear of the crossings' lines first. Among services close either side there may be
            // no such place. Level text written across a line, masked, cannot be taken for that
            // line's note (those stand up along their lines), so it goes across one rather than
            // leave the centreline unlabelled - as a last resort, once the crossings' notes have
            // had their places.
            let mut boxes = clear.clone();
            boxes.extend(across.iter().copied());
            let text = lines.join("\n");
            let mut placed = Placed::new(
                clear,
                -1.0,
                Box::new(move |canvas: &mut dyn SectionCanvas, k: usize| {
                    let place = boxes[k];
                    canvas.knock_out(&place.inflated(0.3));
                    let first = SectionTextStyle {
                        vertical: VerticalJustify::Top,
                        ..note.clone()
                    };
                    canvas.text(Point2::new(place.min.x, place.max.y), &text, &first, 1.0);
                }),
            );
            placed.note = true;
            placed.fallback = across;
            inside.push(placed);
        }
    }

    // Each crossing: its note stood up beside its line, above or below its marker (or from the
    // plot's top or foot without one), in full or as its layer alone - staggered through those
    // places, the nearest the middle placed first; its chainage or offset under the marker.
    let middle = if axes.cross {
        x_of(0.0)
    } else {
        plot.center().x
    };
    let offset_style = SectionTextStyle {
        cap_mm: 1.3,
        ..small.clone()
    };
    for at in &crossings {
        let priority = (at.x - middle).abs();
        let cap = small.cap_mm;
        let mut boxes: Vec<Box2> = Vec::new();
        let mut texts: Vec<String> = Vec::new();
        let mut forms = vec![at.note.text.clone()];
        if at.note.short_text != at.note.text {
            forms.push(at.note.short_text.clone());
        }
        for form in &forms {
            let w = canvas.text_width_mm(form, &small);
            for high in [true, false] {
                for side in [-1.0, 1.0] {
                    let x0 = if side < 0.0 {
                        at.x - 0.4 - cap
                    } else {
                        at.x + 0.4
                    };
                    let foot = match at.y {
                        Some(y) if high => y + 1.2,
                        Some(y) => y - 1.2 - w,
                        None if high => plot.max.y - 1.0 - w,
                        None => plot.min.y + 1.0,
                    };
                    let place = Box2::new(Point2::new(x0, foot), Point2::new(x0 + cap, foot + w));
                    if !on_another_line(&place, at.x) {
                        boxes.push(place);
                        texts.push(form.clone());
                    }
                }
            }
        }
        let note_style = small.clone();
        let drawn_boxes = boxes.clone();
        let mut placed = Placed::new(
            boxes,
            priority,
            Box::new(move |canvas: &mut dyn SectionCanvas, k: usize| {
                // Masked, so the note reads where it crosses a series or a shade.
                canvas.knock_out(&drawn_boxes[k].inflated(0.15));
                draw_in_box(canvas, &drawn_boxes[k], &texts[k], &note_style, true);
            }),
        );
        placed.note = true;
        inside.push(placed);
        if let Some(y) = at.y {
            let along = if at.along.abs() < 0.005 {
                0.0
            } else {
                at.along
            };
            let offset = if axes.cross {
                format_fixed(along, 2)
            } else {
                format!("CH {}", format_fixed(along, 3))
            };
            let w = canvas.text_width_mm(&offset, &offset_style);
            let places = vec![
                Box2::new(
                    Point2::new(at.x - w / 2.0, y - 1.2 - offset_style.cap_mm),
                    Point2::new(at.x + w / 2.0, y - 1.2),
                ),
                Box2::new(
                    Point2::new(at.x - w / 2.0, y + 1.2),
                    Point2::new(at.x + w / 2.0, y + 1.2 + offset_style.cap_mm),
                ),
            ];
            let drawn_places = places.clone();
            let style = offset_style.clone();
            inside.push(Placed::new(
                places,
                priority + 1e-6,
                Box::new(move |canvas: &mut dyn SectionCanvas, k: usize| {
                    canvas.knock_out(&drawn_places[k].inflated(0.2));
                    draw_in_box(canvas, &drawn_places[k], &offset, &style, false);
                }),
            ));
        }
    }
    let dropped = place_and_draw(canvas, &inside, &plot.inflated(-0.3), &[], 0.3);

    // ---- labels outside the plot ----
    // Levels up the left, and under the plot the axis values - or the data band - and a cross
    // section's caption; none outside the viewport.
    let mut outside: Vec<Placed> = Vec::new();
    let mut fixed: Vec<Box2> = Vec::new();
    for &e in &level_values {
        let label = number(e, v_step);
        let w = canvas.text_width_mm(&label, &level_style);
        let place = Box2::new(
            Point2::new(plot.min.x - 0.8 - w, y_of(e) - level_style.cap_mm / 2.0),
            Point2::new(plot.min.x - 0.8, y_of(e) + level_style.cap_mm / 2.0),
        );
        let style = level_style.clone();
        outside.push(Placed::new(
            vec![place],
            -1.0,
            Box::new(move |canvas: &mut dyn SectionCanvas, _: usize| {
                draw_in_box(canvas, &place, &label, &style, false);
            }),
        ));
    }
    if !axes.caption.is_empty() {
        let caption = SectionTextStyle {
            cap_mm: 2.0,
            x_factor: 1.0,
            bold: true,
            horizontal: HorizontalJustify::Centre,
            ..SectionTextStyle::default()
        };
        let room = area.width() - 2.0;
        let mut squeeze = 1.0;
        let w = canvas.text_width_mm(&axes.caption, &caption);
        if w > room && room > 0.0 {
            squeeze = room / w;
        }
        let w = canvas.text_width_mm(&axes.caption, &caption) * squeeze;
        let x = katana_math::cpp::clamp(
            plot.center().x,
            area.min.x + 1.0 + w / 2.0,
            area.max.x - 1.0 - w / 2.0,
        );
        canvas.text(
            Point2::new(x, area.min.y + 1.2),
            &axes.caption,
            &caption,
            squeeze,
        );
        fixed.push(Box2::new(
            Point2::new(x - w / 2.0, area.min.y + 1.2),
            Point2::new(x + w / 2.0, area.min.y + 1.2 + caption.cap_mm),
        ));
    }

    if !banded {
        // Axis values; a range's ends in full, reading inwards from them.
        let mut axis_label =
            |canvas: &mut dyn SectionCanvas, label: String, x: f64, align: i32, priority: f64| {
                let w = canvas.text_width_mm(&label, &values);
                let left = if align < 0 {
                    x
                } else if align > 0 {
                    x - w
                } else {
                    x - w / 2.0
                };
                let place = Box2::new(
                    Point2::new(left, plot.min.y - 0.8 - values.cap_mm),
                    Point2::new(left + w, plot.min.y - 0.8),
                );
                let style = values.clone();
                outside.push(Placed::new(
                    vec![place],
                    priority,
                    Box::new(move |canvas: &mut dyn SectionCanvas, _: usize| {
                        draw_in_box(canvas, &place, &label, &style, false);
                    }),
                ));
            };
        let first = axes.range.map(|range| range.0);
        for &(value, x) in &ends {
            axis_label(
                &mut *canvas,
                format_fixed(value, 3),
                x,
                if Some(value) == first { -1 } else { 1 },
                -2.0,
            );
        }
        for &v in &along_values {
            axis_label(&mut *canvas, number(v, h_step), x_of(v), 0, 0.0);
        }
    }
    place_and_draw(canvas, &outside, area, &fixed, 0.3);

    if !banded {
        return Ok(dropped);
    }

    // ---- the data band: each row's value at every column that has room ----
    let row_h = SECTION_BAND_ROW_MM;
    let band_top = plot.min.y - 1.0;
    let band_bottom = band_top - row_h * rows.len() as f64;
    let band_left = plot.min.x - layout.left_mm + 0.5;
    let head = SectionTextStyle {
        cap_mm: 1.5,
        vertical: VerticalJustify::Middle,
        ..small.clone()
    };
    for (r, row) in rows.iter().enumerate() {
        let top = band_top - r as f64 * row_h;
        let band_pen = canvas.pen(INK, 0.18);
        set_pen(canvas, band_pen);
        let cell_rect = canvas.at_box(&Box2::new(
            Point2::new(band_left, top - row_h),
            Point2::new(plot.max.x, top),
        ));
        canvas.painter().draw_rect(cell_rect);
        line(
            canvas,
            Point2::new(plot.min.x, top),
            Point2::new(plot.min.x, top - row_h),
        );
        let name = if row.cut_fill {
            String::from("CUT/FILL")
        } else {
            match row.series {
                None => String::from("CHAINAGE"),
                Some(series) => series.name.to_uppercase(),
            }
        };
        let room = plot.min.x - band_left - 1.0;
        let mut squeeze = 1.0;
        let w = canvas.text_width_mm(&name, &head);
        if w > room && room > 0.0 {
            squeeze = room / w;
        }
        canvas.text(
            Point2::new(band_left + 0.6, top - row_h / 2.0),
            &name,
            &head,
            squeeze,
        );
    }
    // The columns: a range's ends first, written in full just inside them, then the grid's; a
    // column too near one already placed is left out.
    let mut columns: Vec<(f64, String)> = Vec::new();
    let mut places: Vec<LabelCandidate> = Vec::new();
    let half = cell.cap_mm / 2.0 + 0.1;
    let first = axes.range.map(|range| range.0);
    for &(value, x) in &ends {
        let start = Some(value) == first;
        let left = if start { x + 0.4 } else { x - 0.4 - 2.0 * half };
        columns.push((value, format_fixed(value, 3)));
        places.push(LabelCandidate {
            boxes: vec![Box2::new(
                Point2::new(left, band_bottom),
                Point2::new(left + 2.0 * half, band_top),
            )],
            priority: -1.0,
        });
    }
    for &v in &along_values {
        columns.push((v, number(v, h_step)));
        places.push(LabelCandidate {
            boxes: vec![Box2::new(
                Point2::new(x_of(v) - half, band_bottom),
                Point2::new(x_of(v) + half, band_top),
            )],
            priority: 0.0,
        });
    }
    let placed_columns = place_labels(
        &places,
        &Box2::new(
            Point2::new(plot.min.x, band_bottom),
            Point2::new(plot.max.x, band_top),
        ),
        &[],
        0.6,
    );
    let turned = SectionTextStyle {
        angle_degrees: 90.0,
        vertical: VerticalJustify::Middle,
        horizontal: HorizontalJustify::Centre,
        ..cell.clone()
    };
    for (c, (value, chainage)) in columns.iter().enumerate() {
        if placed_columns[c].is_none() {
            continue;
        }
        let x = places[c].boxes[0].center().x;
        let station = value - axes.shift;
        for (r, row) in rows.iter().enumerate() {
            let text = if row.cut_fill {
                match (design, ground) {
                    (Some(design), Some(ground)) => cut_fill_at(design, ground, station)
                        .map(cut_fill_text)
                        .unwrap_or_default(),
                    _ => String::new(),
                }
            } else {
                match row.series {
                    None => chainage.clone(),
                    Some(series) => level_at(series, station)
                        .map(|level| format_fixed(level, 3))
                        .unwrap_or_default(),
                }
            };
            if text.is_empty() {
                continue;
            }
            // Turned across its row, and never taller than the row.
            let room = row_h - 1.2;
            let mut squeeze = 1.0;
            let w = canvas.text_width_mm(&text, &turned);
            if w > room {
                squeeze = room / w;
            }
            let top = band_top - r as f64 * row_h;
            canvas.text(Point2::new(x, top - row_h / 2.0), &text, &turned, squeeze);
        }
    }
    Ok(dropped)
}

/// Draws LongSection or CrossSections `viewport` into its rectangle less the title's strip, at its
/// own scale and exaggeration (the sheet painter has already put an automatic one's there).
/// `model` gives crossings their own levels.
pub fn paint_section_viewport(
    canvas: &mut dyn SectionCanvas,
    viewport: &Viewport,
    cuts: &mut SectionCuts,
    model: Option<&Model>,
) -> SectionPaintResult {
    let mut result = SectionPaintResult::default();
    let from = &viewport.source;
    if from.alignment.is_empty() {
        result.problems.push(String::from("no alignment chosen"));
        return result;
    }
    // A stored scale comes from JSON unchecked: one that is not a positive number would put every
    // label at infinity.
    if !(viewport.scale > 0.0)
        || !viewport.scale.is_finite()
        || !viewport.vertical_exaggeration.is_finite()
    {
        result.problems.push(format!(
            "the scale 1:{} is not a positive number",
            format_exact_real(viewport.scale)
        ));
        return result;
    }
    // So are the range, the centre and the cross sections' settings: a number that is not finite
    // would put the whole section at infinity.
    let stations_finite = from.stations.iter().all(|station| station.is_finite());
    if !from.chainage_from.is_finite()
        || !from.chainage_to.is_finite()
        || !from.section_interval.is_finite()
        || !from.section_half_width.is_finite()
        || !stations_finite
        || (!viewport.auto_centre
            && (!viewport.centre.x.is_finite() || !viewport.centre.y.is_finite()))
    {
        result.problems.push(String::from(
            "a chainage, centre or width of the section is not a finite number",
        ));
        return result;
    }
    let mut axes = SectionAxes {
        scale: viewport.scale,
        exaggeration: if viewport.vertical_exaggeration > 0.0 {
            viewport.vertical_exaggeration
        } else {
            1.0
        },
        centre: viewport.centre,
        auto_centre: viewport.auto_centre,
        hidden: Some(&viewport.hidden_layers),
        model,
        ..SectionAxes::default()
    };
    let area = section_drawing_area(&viewport.rect);

    if viewport.kind == ViewportKind::LongSection {
        let (section, start) = match (cuts.long_section)() {
            Ok(cut) => cut,
            Err(failure) => {
                result.problems.push(failure);
                return result;
            }
        };
        axes.shift = start;
        // The chainages asked for, else the whole alignment: exactly those are shown, and their
        // ends labelled.
        axes.range = Some(if from.chainage_to > from.chainage_from {
            (from.chainage_from, from.chainage_to)
        } else {
            (start, start + section.length)
        });
        axes.data_band = true;
        match paint_section(canvas, &area, &section, &axes) {
            Ok(dropped) => {
                result.notes_dropped += dropped;
                result.drawn = true;
            }
            Err(failure) => result.problems.push(failure),
        }
        return result;
    }

    // Cross sections: the chainages asked for, one row each.
    let stations = viewport_stations(from);
    if stations.is_empty() {
        result
            .problems
            .push(String::from("no chainage to cut a cross section at"));
        return result;
    }
    let half_width = viewport_half_width(from);
    axes.cross = true;
    let row_height = area.height() / stations.len() as f64;
    for (i, &chainage) in stations.iter().enumerate() {
        let section = match (cuts.cross_section)(chainage, half_width) {
            Ok(section) => section,
            Err(failure) => {
                result.problems.push(failure);
                continue;
            }
        };
        let top = area.max.y - i as f64 * row_height;
        let row = Box2::new(
            Point2::new(area.min.x, top - row_height),
            Point2::new(area.max.x, top),
        );
        let row_axes = SectionAxes {
            shift: -half_width,
            caption: format!("CH {}", format_fixed(chainage, 3)),
            profile_level: cuts
                .profile_level
                .as_ref()
                .and_then(|level| level(chainage)),
            ..axes.clone()
        };
        match paint_section(canvas, &row, &section, &row_axes) {
            Ok(dropped) => {
                result.notes_dropped += dropped;
                result.drawn = true;
            }
            Err(failure) => result
                .problems
                .push(format!("{}: {failure}", row_axes.caption)),
        }
    }
    result
}

/// The scale and exaggeration section `viewport` is drawn at: its own, or with auto_scale those
/// plotting's fit_section chooses for its plot - laid out as paint_section_viewport lays it out,
/// around its measured level labels - for its chainage range (the whole alignment without one) or
/// its cross sections' full width, and the level range of what it shows. A viewport that cannot
/// be cut keeps its own.
pub fn fit_section_viewport(
    canvas: &mut dyn SectionCanvas,
    viewport: &Viewport,
    cuts: &mut SectionCuts,
    model: Option<&Model>,
) -> SectionFit {
    let fit = SectionFit {
        scale: viewport.scale,
        exaggeration: if viewport.vertical_exaggeration > 0.0 {
            viewport.vertical_exaggeration
        } else {
            1.0
        },
    };
    let from = &viewport.source;
    if !viewport.auto_scale || viewport.rect.empty() || from.alignment.is_empty() {
        return fit;
    }
    let area = section_drawing_area(&viewport.rect);
    let centre_along = if viewport.auto_centre {
        None
    } else {
        Some(viewport.centre.x)
    };
    let centre_level = if viewport.auto_centre {
        None
    } else {
        Some(viewport.centre.y)
    };
    // The level column, as wide as the widest level written to a tenth: near enough the painter's
    // own for a fit that leaves a tenth of the plot spare.
    let level_style = level_label_style();
    let level_column = |canvas: &dyn SectionCanvas, levels: (f64, f64)| {
        katana_math::cpp::max(
            canvas.text_width_mm(&number(levels.0, 0.1), &level_style),
            canvas.text_width_mm(&number(levels.1, 0.1), &level_style),
        )
    };

    let mut request = SectionFitRequest::default();
    let layout;
    if viewport.kind == ViewportKind::LongSection {
        let Ok((section, start)) = (cuts.long_section)() else {
            return fit;
        };
        // The range asked for, else the whole alignment.
        let ranged = from.chainage_to > from.chainage_from;
        let first = if ranged { from.chainage_from } else { start };
        let last = if ranged {
            from.chainage_to
        } else {
            start + section.length
        };
        let levels = level_range(
            &section,
            first - start,
            last - start,
            model,
            Some(&viewport.hidden_layers),
        );
        layout = SectionLayoutRequest {
            area,
            band_rows: band_rows(&section).len(),
            caption: false,
            level_label_mm: levels.map_or(0.0, |levels| level_column(&*canvas, levels)),
        };
        request.span_m = section_span(first, last, centre_along);
        request.depth_m = levels.map_or(0.0, |(low, high)| section_span(low, high, centre_level));
        request.flat_exaggeration = LONG_SECTION_EXAGGERATION;
    } else {
        // Every row at one scale: each as wide as the viewport, the deepest setting the
        // exaggeration.
        let stations = viewport_stations(from);
        if stations.is_empty() {
            return fit;
        }
        let half_width = viewport_half_width(from);
        let mut column: f64 = 0.0;
        for &chainage in &stations {
            let Ok(section) = (cuts.cross_section)(chainage, half_width) else {
                continue;
            };
            let levels = widened(
                level_range(
                    &section,
                    f64::NEG_INFINITY,
                    f64::INFINITY,
                    model,
                    Some(&viewport.hidden_layers),
                ),
                cuts.profile_level
                    .as_ref()
                    .and_then(|level| level(chainage)),
            );
            if let Some((low, high)) = levels {
                request.depth_m =
                    katana_math::cpp::max(request.depth_m, section_span(low, high, centre_level));
                column = katana_math::cpp::max(column, level_column(&*canvas, (low, high)));
            }
        }
        let row_height = area.height() / stations.len() as f64;
        layout = SectionLayoutRequest {
            area: Box2::new(area.min, Point2::new(area.max.x, area.min.y + row_height)),
            band_rows: 0,
            caption: true,
            level_label_mm: column,
        };
        request.span_m = section_span(-half_width, half_width, centre_along);
    }
    let plot = section_plot_layout(&layout);
    request.plot_width_mm = plot.plot.width();
    request.plot_height_mm = plot.plot.height();
    fit_section(&request).unwrap_or(fit)
}
