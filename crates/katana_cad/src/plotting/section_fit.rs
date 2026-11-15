//! A section fitted to its viewport, and the steps its labels are written at
//! (docs/plotting.md, "Section smarts";
//! include/katana/cad/plotting/section_fit.hpp).
//!
//! A section viewport with auto_scale is drawn at the largest standard
//! horizontal scale (SHEET_SCALES) at which what it shows fits across its
//! plot - a long section's chainage range, or the whole alignment; a cross
//! section's two half widths - and at the largest exaggeration of the
//! drafter's ladder at which its levels then fit up the plot. The painter
//! measures; everything it decides with the numbers is here, so the choice is
//! tested without a pixel and an agent can ask what a viewport will be drawn
//! at.
//!
//! Labels decide their own spacing: the grid step along a section is the
//! smallest round step (1, 2 or 5 times a power of ten) at which the widest
//! label that step writes still clears its neighbours, and the plot is laid
//! out around the widest level label, so no value is drawn over another or
//! outside its viewport.

use katana_core::error::make_error;
use katana_core::text::format_exact_real;
use katana_core::{ErrorCode, Result};
use katana_geometry::{Box2, Point2};
use katana_math::cpp;

use crate::plot::sheet_scale_at_least;
use crate::plotting::sheet_set::{ViewportSource, std_fixed};

/// The vertical exaggerations a section is drawn at, smallest first: the
/// drafter's ladder. 20 is for the flat long section whose relief would
/// otherwise be a line.
pub const SECTION_EXAGGERATIONS: [f64; 8] = [1.0, 2.0, 2.5, 4.0, 5.0, 8.0, 10.0, 20.0];

/// How much of its plot an automatically scaled section fills, each way: the
/// rest keeps the ground at the extremes off the plot's frame.
pub const SECTION_FIT_FILL: f64 = 0.9;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SectionFitRequest {
    /// How many metres must fit across the plot: a long section's chainages,
    /// a cross section's full width (section_span).
    pub span_m: f64,
    /// How many metres of level must fit up it; 0 when the section is flat or
    /// nothing was sampled.
    pub depth_m: f64,
    /// The plot the section is drawn in, on paper (section_plot_layout).
    pub plot_width_mm: f64,
    pub plot_height_mm: f64,
    pub fill: f64,
    /// The exaggeration when there is no depth to fit: a flat section is not
    /// stretched to the ladder's top.
    pub flat_exaggeration: f64,
}

impl Default for SectionFitRequest {
    fn default() -> Self {
        Self {
            span_m: 0.0,
            depth_m: 0.0,
            plot_width_mm: 0.0,
            plot_height_mm: 0.0,
            fill: SECTION_FIT_FILL,
            flat_exaggeration: 1.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SectionFit {
    /// 1 : scale horizontally.
    pub scale: f64,
    /// The vertical scale is scale / exaggeration.
    pub exaggeration: f64,
}

impl Default for SectionFit {
    fn default() -> Self {
        Self {
            scale: 500.0,
            exaggeration: 1.0,
        }
    }
}

// A choice exactly on a boundary - a span that needs 1 : 500 to the last bit
// - is not pushed to the next step by the rounding of the arithmetic that
// found it.
const RELATIVE_SLACK: f64 = 1e-9;

// The room a level label needs beside the plot: 0.8 mm from the plot's edge
// to the text, and as much again from the text to the viewport's.
const LEVEL_LABEL_CLEARANCE_MM: f64 = 1.6;
// The room a section keeps above and right of its plot.
const PLOT_MARGIN_MM: f64 = 2.0;
// A plot smaller than this each way has nothing worth drawing in it.
const MINIMUM_PLOT_WIDTH_MM: f64 = 10.0;
const MINIMUM_PLOT_HEIGHT_MM: f64 = 8.0;
// Never narrower than this left of the plot, however short the levels.
const MINIMUM_LEFT_MM: f64 = 6.0;

/// How much of an axis a plot must show: `high - low`, or, around a centre
/// the viewport fixes, twice the farther of the two from it. 0 for an empty
/// range.
pub fn section_span(low: f64, high: f64, centre: Option<f64>) -> f64 {
    if !(high >= low) || !low.is_finite() || !high.is_finite() {
        return 0.0;
    }
    if let Some(centre) = centre
        && centre.is_finite()
    {
        return 2.0 * cpp::max((high - centre).abs(), (centre - low).abs());
    }
    high - low
}

/// The largest of SECTION_EXAGGERATIONS at which `depth_m` metres at
/// 1 : `scale` fits `height_mm` and the vertical scale, 1 : scale / it, has a
/// whole denominator (at 1 : 750, 2.5 - V 1:300 - rather than 4 - V 1:187.5);
/// the largest that fits when none of those does; 1 when even that does not
/// fit, and `flat_exaggeration` when there is no depth (the C++ defaults it
/// to 1).
pub fn fit_exaggeration(depth_m: f64, scale: f64, height_mm: f64, flat_exaggeration: f64) -> f64 {
    if !(depth_m > 0.0) || !depth_m.is_finite() {
        return flat_exaggeration;
    }
    // The largest that fits, among those that give a vertical scale a rule
    // reads - a whole denominator (1 : 750 at 2.5 is V 1:300, at 4 would be
    // V 1:187.5) - else the largest that fits at all.
    let mut best = SECTION_EXAGGERATIONS[0];
    let mut whole: Option<f64> = None;
    for candidate in SECTION_EXAGGERATIONS {
        if depth_m * 1000.0 * candidate / scale <= height_mm {
            best = candidate;
            let vertical = scale / candidate;
            if (vertical - vertical.round()).abs() <= 1e-9 * vertical {
                whole = Some(candidate);
            }
        }
    }
    whole.unwrap_or(best)
}

/// The scale is the largest standard one at which the span fits the plot's
/// width, and at which the depth still fits its height at exaggeration 1 - a
/// deep, narrow section is drawn smaller rather than out of its plot. The
/// exaggeration is then the largest of SECTION_EXAGGERATIONS at which the
/// depth fits the height (fit_exaggeration). InvalidArgument for a span that
/// is not positive and finite, a plot with no size, a fill outside (0, 1], or
/// a negative depth.
pub fn fit_section(request: &SectionFitRequest) -> Result<SectionFit> {
    if !(request.span_m > 0.0) || !request.span_m.is_finite() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "a section needs a length to fit",
            format!("{} m", format_exact_real(request.span_m)),
        ));
    }
    if !(request.plot_width_mm > 0.0)
        || !(request.plot_height_mm > 0.0)
        || !request.plot_width_mm.is_finite()
        || !request.plot_height_mm.is_finite()
    {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "the section's plot has no room",
            format!(
                "{} x {} mm",
                format_exact_real(request.plot_width_mm),
                format_exact_real(request.plot_height_mm)
            ),
        ));
    }
    if !(request.fill > 0.0) || request.fill > 1.0 {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "the fill must be above 0 and at most 1",
            format_exact_real(request.fill),
        ));
    }
    if !(request.depth_m >= 0.0) || !request.depth_m.is_finite() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "the depth must not be negative",
            format!("{} m", format_exact_real(request.depth_m)),
        ));
    }
    let width = request.plot_width_mm * request.fill;
    let height = request.plot_height_mm * request.fill;
    // Across, and - at true scale - up: a deep narrow section is drawn
    // smaller rather than over its frame.
    let mut needed = request.span_m * 1000.0 / width;
    if request.depth_m > 0.0 {
        needed = cpp::max(needed, request.depth_m * 1000.0 / height);
    }
    let scale = sheet_scale_at_least(needed * (1.0 - RELATIVE_SLACK))?;
    Ok(SectionFit {
        scale,
        exaggeration: fit_exaggeration(
            request.depth_m,
            scale,
            height * (1.0 + RELATIVE_SLACK),
            request.flat_exaggeration,
        ),
    })
}

// ---- label steps ---------------------------------------------------------------------

/// The first round step (1, 2 or 5 times a power of ten) at least `minimum`;
/// 1 for a minimum that is not positive and finite.
pub fn round_step_at_least(minimum: f64) -> f64 {
    if !(minimum > 0.0) || !minimum.is_finite() {
        return 1.0;
    }
    let magnitude = 10.0_f64.powf(minimum.log10().floor());
    for multiple in [1.0, 2.0, 5.0, 10.0] {
        if magnitude * multiple >= minimum * (1.0 - RELATIVE_SLACK) {
            return magnitude * multiple;
        }
    }
    magnitude * 10.0
}

/// The round step after `step`: 1 -> 2 -> 5 -> 10.
pub fn next_round_step(step: f64) -> f64 {
    if !(step > 0.0) || !step.is_finite() {
        return 1.0;
    }
    let magnitude = 10.0_f64.powf((step.log10() + RELATIVE_SLACK).floor());
    let multiple = step / magnitude;
    if multiple < 1.5 {
        return 2.0 * magnitude;
    }
    if multiple < 3.5 {
        return 5.0 * magnitude;
    }
    10.0 * magnitude
}

/// How many decimals a value on a grid of `step` is written with: none for a
/// step of 1 or more, else enough for the step (0.5 -> 1, 0.25 -> 2), at most 3.
pub fn step_decimals(step: f64) -> i32 {
    if !(step > 0.0) || !step.is_finite() {
        return 0;
    }
    for decimals in 0..3 {
        // std::pow(double, int) is pow on two doubles.
        let scaled = step * 10.0_f64.powf(f64::from(decimals));
        if (scaled - scaled.round()).abs() <= 1e-6 * cpp::max(1.0, scaled) {
            return decimals;
        }
    }
    3
}

/// `value` as a label on a grid of `step` ("125", "12.5"); never "-0".
pub fn step_text(value: f64, step: f64) -> String {
    let decimals = step_decimals(step);
    // A value that rounds to zero is written "0", never "-0".
    let value = if value.abs() < 0.5 * 10.0_f64.powf(f64::from(-decimals)) {
        0.0
    } else {
        value
    };
    std_fixed(value, decimals as usize)
}

/// The C++'s default for grid_values' `cap`.
pub const GRID_VALUES_CAP: usize = 400;

/// The grid values of `step` in [first, last], at most `cap` of them.
pub fn grid_values(first: f64, last: f64, step: f64, cap: usize) -> Vec<f64> {
    let mut values = Vec::new();
    if !(step > 0.0) || !(last >= first) || !first.is_finite() || !last.is_finite() {
        return values;
    }
    // Counted from a whole number of steps, so the thousandth value is not
    // a thousand additions' rounding away from its multiple.
    let first_index = (first / step - RELATIVE_SLACK).ceil();
    let mut i: usize = 0;
    while values.len() < cap {
        let value = (first_index + i as f64) * step;
        if value > last + step * RELATIVE_SLACK {
            break;
        }
        values.push(value);
        i += 1;
    }
    values
}

/// The smallest round step at which labels along an axis drawn at
/// `mm_per_unit` never overlap: the grid is at least `minimum_spacing_mm`
/// apart and neighbouring labels, `widest_label_mm(step)` wide at most, leave
/// `gap_mm` between them. A label written across the axis (a data band's,
/// turned 90 degrees) passes its height as its width. The widest label is
/// asked of each step tried, since a finer step writes more decimals.
pub fn label_step(
    mm_per_unit: f64,
    minimum_spacing_mm: f64,
    gap_mm: f64,
    widest_label_mm: Option<&dyn Fn(f64) -> f64>,
) -> f64 {
    if !(mm_per_unit > 0.0) || !mm_per_unit.is_finite() {
        return 1.0;
    }
    let mut step = round_step_at_least(minimum_spacing_mm / mm_per_unit);
    // Sixty steps is eighteen decades: far past any label a sheet can hold.
    for _ in 0..60 {
        let widest = widest_label_mm.map_or(0.0, |widest| widest(step));
        if step * mm_per_unit >= (widest + gap_mm) * (1.0 - RELATIVE_SLACK) {
            return step;
        }
        step = next_round_step(step);
    }
    step
}

// ---- the plot's layout ----------------------------------------------------------------

/// Paper room around a section's plot, in millimetres: a data band row.
pub const SECTION_BAND_ROW_MM: f64 = 8.0;
/// The band's row names.
pub const SECTION_BAND_HEAD_MM: f64 = 22.0;
/// The axis values under a plot.
pub const SECTION_AXIS_ROW_MM: f64 = 4.0;
/// A cross section's "CH" caption.
pub const SECTION_CAPTION_MM: f64 = 4.5;
/// Less: no band.
pub const SECTION_MINIMUM_BAND_PLOT_MM: f64 = 30.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SectionLayoutRequest {
    /// The viewport, less its title's corner.
    pub area: Box2,
    /// Data band rows wanted (a long section's); 0: none.
    pub band_rows: usize,
    /// A caption under it (a cross section's chainage).
    pub caption: bool,
    /// The widest level label, drawn left of the plot.
    pub level_label_mm: f64,
}

impl Default for SectionLayoutRequest {
    fn default() -> Self {
        Self {
            area: Box2::default(),
            band_rows: 0,
            caption: false,
            level_label_mm: 9.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SectionLayout {
    /// Where the section itself is drawn.
    pub plot: Box2,
    /// The data band fits under it.
    pub banded: bool,
    /// The room left of the plot: level labels or band names.
    pub left_mm: f64,
}

/// Where a section's plot goes in `area`: the level labels to its left (as
/// wide as the widest, and the band's names when there is a band), the band
/// or the axis values and the caption below. A band is dropped when it would
/// leave the plot less than SECTION_MINIMUM_BAND_PLOT_MM high; the axis values
/// take its place. An empty plot when the area is too small for any.
pub fn section_plot_layout(request: &SectionLayoutRequest) -> SectionLayout {
    let mut layout = SectionLayout::default();
    let area = &request.area;
    if area.empty() {
        return layout;
    }
    let levels = cpp::max(
        request.level_label_mm + LEVEL_LABEL_CLEARANCE_MM,
        MINIMUM_LEFT_MM,
    );
    let caption = if request.caption {
        SECTION_CAPTION_MM
    } else {
        0.0
    };
    let plot_with = |left: f64, bottom: f64| {
        Box2::new(
            Point2::new(area.min.x + left, area.min.y + bottom + 1.0),
            Point2::new(area.max.x - PLOT_MARGIN_MM, area.max.y - PLOT_MARGIN_MM),
        )
    };
    if request.band_rows > 0 {
        // The band's row names share the column left of the plot with the
        // level labels, so it is as wide as the wider.
        let left = cpp::max(SECTION_BAND_HEAD_MM, levels);
        let plot = plot_with(
            left,
            request.band_rows as f64 * SECTION_BAND_ROW_MM + caption,
        );
        if plot.height() >= SECTION_MINIMUM_BAND_PLOT_MM {
            layout.plot = plot;
            layout.banded = true;
            layout.left_mm = left;
        }
    }
    if !layout.banded {
        layout.plot = plot_with(levels, SECTION_AXIS_ROW_MM + caption);
        layout.left_mm = levels;
    }
    if !(layout.plot.width() > MINIMUM_PLOT_WIDTH_MM)
        || !(layout.plot.height() > MINIMUM_PLOT_HEIGHT_MM)
    {
        return SectionLayout::default();
    }
    layout
}

// ---- a section viewport ----------------------------------------------------------------

/// The strip at a section viewport's foot kept for its title.
pub const SECTION_TITLE_MM: f64 = 7.0;
/// A cross section's half width when the viewport gives none.
pub const DEFAULT_SECTION_HALF_WIDTH: f64 = 20.0;
/// At most so many cross sections are cut for one viewport's interval.
pub const MAXIMUM_SECTION_ROWS: usize = 64;

/// Where a section viewport draws its sections: its rectangle less the
/// title's strip.
pub fn section_drawing_area(rect: &Box2) -> Box2 {
    if rect.empty() {
        return *rect;
    }
    let mut area = *rect;
    area.min.y = cpp::min(area.min.y + SECTION_TITLE_MM, area.max.y);
    area
}

/// The chainages a CrossSections viewport cuts at, one row each, top down:
/// its stations, else every interval from its first chainage to its last
/// (MAXIMUM_SECTION_ROWS at most).
pub fn viewport_stations(source: &ViewportSource) -> Vec<f64> {
    let mut stations = source.stations.clone();
    if stations.is_empty()
        && source.section_interval > 0.0
        && source.chainage_to > source.chainage_from
    {
        // Counted in whole intervals, so the last is not an accumulation of
        // additions' rounding short of the range's end.
        let mut i: usize = 0;
        while stations.len() < MAXIMUM_SECTION_ROWS {
            let at = source.chainage_from + i as f64 * source.section_interval;
            if at > source.chainage_to + 1e-9 {
                break;
            }
            stations.push(at);
            i += 1;
        }
    }
    stations
}

/// Its sections' half width: its own, else DEFAULT_SECTION_HALF_WIDTH.
pub fn viewport_half_width(source: &ViewportSource) -> f64 {
    if source.section_half_width > 0.0 && source.section_half_width.is_finite() {
        source.section_half_width
    } else {
        DEFAULT_SECTION_HALF_WIDTH
    }
}
