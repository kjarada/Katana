//! Plotting: the sheet, the scale, and what a millimetre of paper is
//! (include/katana/cad/plot.hpp).
//!
//! This is what makes `Layer::line_weight` true. It has been documented as
//! "millimetres on paper", validated and persisted, and read by nothing that
//! draws, because there was no paper for it to be millimetres of. A plot is
//! the same drawing code the viewport uses, painted through a sheet transform
//! instead of the screen's, with the pen width taken from the line weight -
//! so a line weight does exactly what it says, and the viewport and the plot
//! cannot disagree about anything but the paper.
//!
//! Everything here is arithmetic on the sheet and is tested as such; the PDF
//! writer and the painter live in the window layer, which calls this for the
//! numbers.
//!
//! Model units are metres, as everywhere in the product. A plot at 1 : N puts
//! one metre on the paper as 1000 / N millimetres.

use katana_core::error::make_error;
use katana_core::text::{format_exact_real, lowered};
use katana_core::{Error, ErrorCode, Result};
use katana_entity::{Color, EnumName};
use katana_geometry::{Box2, Point2};
use katana_math::cpp;

use crate::view_transform::ViewTransform;

/// ISO 216 A series, portrait, in millimetres (ISO 216:2007, Table 1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PaperSize {
    A0,
    A1,
    A2,
    A3,
    A4,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PaperDimensions {
    pub width_mm: f64,
    pub height_mm: f64,
}

pub fn paper_dimensions(size: PaperSize, landscape: bool) -> PaperDimensions {
    // ISO 216:2007 Table 1. Each size is the previous halved along its long
    // side; A0 is one square metre, rounded to the millimetre.
    let (width_mm, height_mm) = match size {
        PaperSize::A0 => (841.0, 1189.0),
        PaperSize::A1 => (594.0, 841.0),
        PaperSize::A2 => (420.0, 594.0),
        PaperSize::A3 => (297.0, 420.0),
        PaperSize::A4 => (210.0, 297.0),
    };
    if landscape {
        PaperDimensions {
            width_mm: height_mm,
            height_mm: width_mm,
        }
    } else {
        PaperDimensions {
            width_mm,
            height_mm,
        }
    }
}

/// The scale ladder a survey drawing is plotted at, as denominators. A fitted
/// plot picks the first of these the drawing fits at, so a sheet is 1 : 2000
/// and not 1 : 1873 - a scale bar is only useful when the scale is one a
/// scale rule carries.
pub const STANDARD_SCALES: [f64; 12] = [
    100.0, 200.0, 250.0, 500.0, 1000.0, 2000.0, 2500.0, 5000.0, 10000.0, 20000.0, 25000.0, 50000.0,
];

/// How a plot's colours print: the plot style's colour mode (docs/plotting.md,
/// "Plot styles and output"). Stored by name ("colour", "greyscale",
/// "monochrome"), never by number; a new mode goes at the end, and a name is
/// never changed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PlotColourMode {
    /// As drawn, white printing black (white_to_black).
    #[default]
    Colour,
    /// Every colour as its luminance: a grey as light as it was.
    Greyscale,
    /// Every line and letter black; a fill black or white by its lightness.
    Monochrome,
}

/// The C++ `toString(PlotColourMode)`.
impl EnumName for PlotColourMode {
    fn name(self) -> &'static str {
        match self {
            Self::Colour => "colour",
            Self::Greyscale => "greyscale",
            Self::Monochrome => "monochrome",
        }
    }
}

/// The name EnumName gives, or a common other spelling: "color", "grey",
/// "gray", "grayscale", "mono", "black". Case is ignored.
pub fn plot_colour_mode_from(name: &str) -> Option<PlotColourMode> {
    match lowered(name).as_slice() {
        b"colour" | b"color" => Some(PlotColourMode::Colour),
        b"greyscale" | b"grayscale" | b"grey" | b"gray" => Some(PlotColourMode::Greyscale),
        b"monochrome" | b"mono" | b"black" => Some(PlotColourMode::Monochrome),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlotSettings {
    pub paper: PaperSize,
    pub landscape: bool,
    /// All round; the drawing is fitted inside it.
    pub margin_mm: f64,
    /// Device resolution. 300 dpi is what a PDF viewer and a plotter both
    /// resolve; higher makes every hatch line and label cost more for no
    /// visible gain.
    pub dpi: f64,
    /// 1 : N.
    pub scale_denominator: f64,
    /// The model point at the sheet centre.
    pub center: Point2,
    /// White and near-white pens print BLACK (decision D7), as AutoCAD's
    /// colour 7 does. The screen's ground is dark, so white is what a new
    /// layer draws in and what 130 of the 457 map_data rules of a reference
    /// mapfile ask for; on white paper every one of those features would
    /// vanish. Off only for a plot onto a dark sheet or a deliberate
    /// white-on-colour print.
    pub white_to_black: bool,
    /// The plot style. The colour mode is applied by paper_colour and
    /// paper_fill_colour, which every paper colour passes through, so a plan,
    /// a section and the frame all follow it. The line weight scale
    /// multiplies every pen width on paper - a check plot at 0.7, a bold one
    /// at 1.4 - and leaves text, dash lengths and symbol sizes as they are.
    pub colour_mode: PlotColourMode,
    pub line_weight_scale: f64,
}

impl Default for PlotSettings {
    fn default() -> Self {
        Self {
            paper: PaperSize::A3,
            landscape: true,
            margin_mm: 10.0,
            dpi: 300.0,
            scale_denominator: 1000.0,
            center: Point2::new(0.0, 0.0),
            white_to_black: true,
            colour_mode: PlotColourMode::Colour,
            line_weight_scale: 1.0,
        }
    }
}

/// The lowest a channel may be for a colour to count as white on paper. 230
/// of 255 is within a tenth of white on every channel: a white a screen has
/// been dimmed or tinted towards (240, 240, 240) still prints, while "light
/// grey" (211, 211, 211) and "light yellow" (255, 255, 224) keep their
/// colour: on paper they are faint but they are there, and they are what was
/// asked for.
pub const NEAR_WHITE_CHANNEL: u8 = 230;

/// A colour's lightness, 0 (black) to 255 (white): the Rec. 601 luma,
/// (299 R + 587 G + 114 B) / 1000, rounded - the weights greyscale printing
/// and image tools use for a colour's grey. In integers, so the same colour
/// gives the same grey on every machine; a grey is its own luminance.
pub fn luminance(colour: Color) -> u8 {
    let weighted = 299_u32 * u32::from(colour.r)
        + 587_u32 * u32::from(colour.g)
        + 114_u32 * u32::from(colour.b);
    ((weighted + 500) / 1000) as u8
}

/// A monochrome plot's fill is black when its luminance is below this, and
/// white paper otherwise: the darker half of the colours print solid and the
/// lighter half drop out, as they would on a one-ink plotter, leaving their
/// outline (a pen, so black) to show the area.
pub const MONOCHROME_FILL_THRESHOLD: u8 = 128;

/// The colour a pen prints in. First the white rule: black for white and
/// near-white when `settings.white_to_black`. Then the colour mode: Colour
/// keeps the colour, Greyscale makes it the grey of its luminance, Monochrome
/// makes it black. Alpha is kept, so a faded pen stays faded.
pub fn paper_colour(colour: Color, settings: &PlotSettings) -> Color {
    let mut colour = colour;
    let near_white = colour.r >= NEAR_WHITE_CHANNEL
        && colour.g >= NEAR_WHITE_CHANNEL
        && colour.b >= NEAR_WHITE_CHANNEL;
    if settings.white_to_black && near_white {
        colour.r = 0;
        colour.g = 0;
        colour.b = 0;
    }
    match settings.colour_mode {
        PlotColourMode::Colour => {}
        PlotColourMode::Greyscale => {
            let grey = luminance(colour);
            colour.r = grey;
            colour.g = grey;
            colour.b = grey;
        }
        PlotColourMode::Monochrome => {
            colour.r = 0;
            colour.g = 0;
            colour.b = 0;
        }
    }
    colour
}

/// The colour an area fill prints in. Colour and Greyscale print a fill as a
/// pen of its colour would (paper_colour), so it keeps its lightness as a
/// grey; Monochrome prints it black or white by MONOCHROME_FILL_THRESHOLD.
/// The white of the paper itself is never passed through this: it stays
/// white in every mode.
pub fn paper_fill_colour(colour: Color, settings: &PlotSettings) -> Color {
    if settings.colour_mode != PlotColourMode::Monochrome {
        return paper_colour(colour, settings);
    }
    let colour_only = PlotSettings {
        colour_mode: PlotColourMode::Colour,
        ..*settings
    };
    let ink = paper_colour(colour, &colour_only); // the white rule only
    let level = if luminance(ink) < MONOCHROME_FILL_THRESHOLD {
        0
    } else {
        255
    };
    Color {
        r: level,
        g: level,
        b: level,
        a: colour.a,
    }
}

/// The sheet as a view transform: pixels here are device pixels of the plot
/// at `dpi`, so `view.scale` is device pixels per model unit and the width
/// and height are the whole paper.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sheet {
    pub view: ViewTransform,
    /// What a line weight is multiplied by.
    pub pixels_per_millimetre: f64,
    pub width_pixels: f64,
    pub height_pixels: f64,
}

// The area the drawing may occupy, in millimetres.
struct Printable {
    width_mm: f64,
    height_mm: f64,
}

fn printable_area(settings: &PlotSettings) -> Result<Printable> {
    if !settings.margin_mm.is_finite() || settings.margin_mm < 0.0 {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the margin must be zero or positive",
        ));
    }
    let paper = paper_dimensions(settings.paper, settings.landscape);
    let area = Printable {
        width_mm: paper.width_mm - 2.0 * settings.margin_mm,
        height_mm: paper.height_mm - 2.0 * settings.margin_mm,
    };
    if !(area.width_mm > 0.0) || !(area.height_mm > 0.0) {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "the margins leave no printable area on the sheet",
            format!(
                "{} mm on {} x {}",
                format_exact_real(settings.margin_mm),
                format_exact_real(paper.width_mm),
                format_exact_real(paper.height_mm)
            ),
        ));
    }
    Ok(area)
}

/// Fails with InvalidArgument for a non-positive resolution or scale, a
/// margin that leaves no printable area, or a non-finite centre.
pub fn sheet_for(settings: &PlotSettings) -> Result<Sheet> {
    if !(settings.dpi > 0.0) || !settings.dpi.is_finite() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the resolution must be positive",
        ));
    }
    if !(settings.scale_denominator > 0.0) || !settings.scale_denominator.is_finite() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the scale denominator must be positive",
        ));
    }
    if !settings.center.x.is_finite() || !settings.center.y.is_finite() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the sheet centre must be finite",
        ));
    }
    printable_area(settings)?;
    let paper = paper_dimensions(settings.paper, settings.landscape);
    let pixels_per_millimetre = millimetres_to_pixels(1.0, settings.dpi);
    let width_pixels = paper.width_mm * pixels_per_millimetre;
    let height_pixels = paper.height_mm * pixels_per_millimetre;
    let mut view = ViewTransform {
        center: settings.center,
        // One metre is 1000 / N millimetres on the paper, and each of those
        // is pixels_per_millimetre device pixels.
        scale: (1000.0 / settings.scale_denominator) * pixels_per_millimetre,
        ..ViewTransform::default()
    };
    view.resize(width_pixels, height_pixels);
    Ok(Sheet {
        view,
        pixels_per_millimetre,
        width_pixels,
        height_pixels,
    })
}

/// The smallest standard denominator at which `extent` fits inside the
/// printable area of the sheet, or the exact denominator needed when even
/// 1 : 50 000 is too large - a plot always fits; it is only the scale bar
/// that then has nothing standard to say. A line along one axis fits on its
/// length: a zero dimension asks nothing of the sheet. Fails with
/// InvalidArgument for an extent with no size in either direction (empty, or
/// one point) or a sheet with no printable area.
pub fn fit_scale(extent: &Box2, settings: &PlotSettings) -> Result<f64> {
    let width_m = extent.max.x - extent.min.x;
    let height_m = extent.max.y - extent.min.y;
    // A line along one axis - a centreline, a row of points on one northing -
    // has no height, and its zero dimension simply asks nothing of the sheet
    // (audit CAD-15), as ViewTransform::fit treats it. A box with no size in
    // either direction - nothing, or one point - has no scale to find.
    if !(width_m >= 0.0)
        || !(height_m >= 0.0)
        || !(cpp::max(width_m, height_m) > 0.0)
        || !width_m.is_finite()
        || !height_m.is_finite()
    {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the drawing has no extent to fit",
        ));
    }
    let area = printable_area(settings)?;
    // The denominator at which each dimension just fits: metres to
    // millimetres, over the millimetres available.
    let needed = cpp::max(
        width_m * 1000.0 / area.width_mm,
        height_m * 1000.0 / area.height_mm,
    );
    for standard in STANDARD_SCALES {
        if standard >= needed {
            return Ok(standard);
        }
    }
    Ok(needed)
}

/// The finer ladder a SHEET's viewports are scaled on (docs/plotting.md). A
/// sheet holds details and sections as well as plans, and a cross section at
/// 1 : 100 on a 120 mm cell is 12 m wide - so it needs 1 : 10 to 1 : 75 and
/// the in-between 1 : 125, 1 : 150, 1 : 750 and 1 : 1250 that STANDARD_SCALES
/// leaves out, and 1 : 1 to 1 : 5 for a detail. STANDARD_SCALES stays as it
/// is: the single-page plot and its tests are built on it.
pub const SHEET_SCALES: [f64; 24] = [
    1.0, 2.0, 5.0, 10.0, 20.0, 25.0, 50.0, 75.0, 100.0, 125.0, 150.0, 200.0, 250.0, 500.0, 750.0,
    1000.0, 1250.0, 2000.0, 2500.0, 5000.0, 10000.0, 20000.0, 25000.0, 50000.0,
];

/// The first denominator of SHEET_SCALES that is at least `needed` (the
/// largest standard scale at which something needing 1 : `needed` still
/// fits), or `needed` itself beyond 1 : 50 000, as fit_scale does.
/// InvalidArgument for a denominator that is not positive and finite.
pub fn sheet_scale_at_least(needed: f64) -> Result<f64> {
    if !(needed > 0.0) || !needed.is_finite() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "the scale denominator must be positive",
            format_exact_real(needed),
        ));
    }
    for standard in SHEET_SCALES {
        if standard >= needed {
            return Ok(standard);
        }
    }
    Ok(needed)
}

pub const fn millimetres_to_pixels(millimetres: f64, dpi: f64) -> f64 {
    millimetres * dpi / 25.4 // 25.4 mm to the inch, by definition
}
