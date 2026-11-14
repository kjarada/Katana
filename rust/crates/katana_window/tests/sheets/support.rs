//! What the sheet twins share: a sheet painted into an image at 4 px a millimetre, the pixel of a
//! paper point, and the counts the C++ tests take of a paper box - ink, a colour, text.

#![allow(
    dead_code,
    reason = "Each file of the target uses only some of these helpers."
)]

use katana_cad::plot::{PlotColourMode, paper_dimensions};
use katana_cad::plotting::sheet_set::SheetSet;
use katana_entity::{Entity, Geometry, Model};
use katana_geometry::{Box2, Point2, Segment2};
use katana_window::canvas::{Canvas, Color, Image, IntRect, RasterCanvas, write_png};
use katana_window::sheet_painter::{
    SheetPaintCache, SheetPaintOptions, SheetPaintStats, SheetSource, paint_sheet,
};

/// ~100 dpi: every 0.25 mm line is a pixel.
pub const PPMM: f64 = 4.0;
pub const WHITE: Color = Color::WHITE;
/// Qt::gray, what a test's image holds before a sheet is painted on it.
pub const GREY: Color = Color::rgb(160, 160, 164);

pub fn bx(x0: f64, y0: f64, x1: f64, y1: f64) -> Box2 {
    Box2::new(Point2::new(x0, y0), Point2::new(x1, y1))
}

/// EXPECT_DOUBLE_EQ: within four units in the last place.
pub fn double_eq(a: f64, b: f64) -> bool {
    if a == b {
        return true;
    }
    if a.is_nan() || b.is_nan() || a.is_sign_negative() != b.is_sign_negative() {
        return false;
    }
    a.to_bits().abs_diff(b.to_bits()) <= 4
}

/// EXPECT_NEAR.
pub fn near(a: f64, b: f64, tolerance: f64) -> bool {
    (a - b).abs() <= tolerance
}

pub fn add_line(model: &mut Model, a: Point2, b: Point2) {
    let entity = Entity {
        geometry: Geometry::Line(Segment2::new(a, b)),
        layer: String::from("0"),
        ..Entity::default()
    };
    model.entities.add(entity).expect("the line is added");
}

/// `paint` run on a raster canvas over `image` (QPainter painter(&image) and what follows it).
pub fn paint_on(image: &mut Image, mut paint: impl FnMut(&mut dyn Canvas)) {
    RasterCanvas::paint_image(image, &mut paint);
}

/// With KATANA_SHEET_PNG set to a directory, a painted sheet is also written there as
/// `<name>.png`, to be looked at.
pub fn keep(image: &Image, name: &str) {
    if let Ok(folder) = std::env::var("KATANA_SHEET_PNG") {
        let _ = write_png(image, &std::path::Path::new(&folder).join(format!("{name}.png")));
    }
}

/// How a test's sheet is painted, beyond the set and the source.
#[derive(Clone, Debug)]
pub struct Painting {
    pub index: usize,
    pub construction: bool,
    pub slot_hints: bool,
    pub colour_mode: PlotColourMode,
    pub line_weight_scale: f64,
    /// The file KATANA_SHEET_PNG keeps the sheet under.
    pub name: String,
}

impl Painting {
    pub fn named(name: &str) -> Self {
        Self {
            index: 0,
            construction: false,
            slot_hints: false,
            colour_mode: PlotColourMode::Colour,
            line_weight_scale: 1.0,
            name: String::from(name),
        }
    }
}

/// Sheet `how.index` of `set` painted at PPMM over a grey image, with what the paint reported.
pub fn painted_with(
    set: &SheetSet,
    source: &SheetSource<'_>,
    how: &Painting,
) -> (Image, SheetPaintStats) {
    let sheet = &set.sheets[how.index];
    let paper = paper_dimensions(sheet.paper, sheet.landscape);
    let mut image = Image::new(
        (paper.width_mm * PPMM).round() as u32,
        (paper.height_mm * PPMM).round() as u32,
    );
    image.fill(GREY);
    let mut options = SheetPaintOptions {
        pixels_per_millimetre: PPMM,
        construction: how.construction,
        slot_hints: how.slot_hints,
        ..SheetPaintOptions::default()
    };
    options.plot.colour_mode = how.colour_mode;
    options.plot.line_weight_scale = how.line_weight_scale;
    let mut cache = SheetPaintCache::new();
    let mut stats = SheetPaintStats::default();
    paint_on(&mut image, |canvas| {
        stats = paint_sheet(canvas, set, how.index, source, &options, &mut cache);
    });
    keep(&image, &how.name);
    (image, stats)
}

/// The set's first sheet, painted as it plots.
pub fn painted(set: &SheetSet, source: &SheetSource<'_>, name: &str) -> (Image, SheetPaintStats) {
    painted_with(set, source, &Painting::named(name))
}

/// The device pixel of a paper point on an image painted by `painted`, for a paper `height_mm`
/// high.
pub fn pixel_of_on(paper: Point2, height_mm: f64) -> (i32, i32) {
    (
        (paper.x * PPMM).floor() as i32,
        ((height_mm - paper.y) * PPMM).floor() as i32,
    )
}

/// The device pixel of a paper point on an A3 landscape sheet.
pub fn pixel_of(paper: Point2) -> (i32, i32) {
    pixel_of_on(paper, 297.0)
}

/// The pixels of a paper box, corner to corner as QRect(topLeft, bottomRight) holds them.
pub fn pixels_of_on(paper: &Box2, height_mm: f64) -> IntRect {
    let a = pixel_of_on(Point2::new(paper.min.x, paper.max.y), height_mm);
    let b = pixel_of_on(Point2::new(paper.max.x, paper.min.y), height_mm);
    IntRect::new(a.0, a.1, b.0 - a.0 + 1, b.1 - a.1 + 1)
}

pub fn pixels_of(paper: &Box2) -> IntRect {
    pixels_of_on(paper, 297.0)
}

/// Pixels in the paper box, cut to the image, that pass `keep`.
pub fn pixels_in_on(
    image: &Image,
    paper: &Box2,
    height_mm: f64,
    keep: impl Fn(Color) -> bool,
) -> usize {
    let r = pixels_of_on(paper, height_mm);
    let mut count = 0;
    for y in r.y.max(0)..=r.bottom().min(image.height() as i32 - 1) {
        for x in r.x.max(0)..=r.right().min(image.width() as i32 - 1) {
            if keep(image.pixel(x as u32, y as u32)) {
                count += 1;
            }
        }
    }
    count
}

pub fn pixels_in(image: &Image, paper: &Box2, keep: impl Fn(Color) -> bool) -> usize {
    pixels_in_on(image, paper, 297.0, keep)
}

/// Pixels in the paper box that are not white.
pub fn ink_in(image: &Image, paper: &Box2) -> usize {
    pixels_in(image, paper, |c| c != WHITE)
}

pub fn ink_in_on(image: &Image, paper: &Box2, height_mm: f64) -> usize {
    pixels_in_on(image, paper, height_mm, |c| c != WHITE)
}

/// Whether every channel of `c` is within `tolerance` of `colour`'s.
pub fn close_to(c: Color, colour: Color, tolerance: u8) -> bool {
    c.r.abs_diff(colour.r) <= tolerance
        && c.g.abs_diff(colour.g) <= tolerance
        && c.b.abs_diff(colour.b) <= tolerance
}

/// Pixels in the paper box within `tolerance` of `colour` on every channel.
pub fn colour_in(image: &Image, paper: &Box2, colour: Color, tolerance: u8) -> usize {
    pixels_in(image, paper, |c| close_to(c, colour, tolerance))
}

/// Text: dark and neutral. At 4 px a millimetre a 1.4 mm letter is mostly its antialiased edge, so
/// dark is anything under 150; nothing else in a section is both - the grid is a light grey
/// hairline, a crossing's line a faint grey dash, each series in its own colour.
pub fn text_in(image: &Image, paper: &Box2) -> usize {
    pixels_in(image, paper, |c| {
        let high = c.r.max(c.g).max(c.b);
        let low = c.r.min(c.g).min(c.b);
        high <= 150 && high - low <= 25
    })
}

/// The pixels of `rect` of `image`, for comparing two paints of one place (QImage::copy).
pub fn copy_of(image: &Image, rect: IntRect) -> Image {
    image.copy(rect)
}

// ---- a dialog on the harness -------------------------------------------------------------------

/// A dialog the harness draws with a host.
pub trait Drawn {
    fn draw(&mut self, ui: &mut egui::Ui, host: &mut dyn katana_window::host::Host);
}

impl Drawn for katana_window::plotting::plot_drawing_dialog::PlotDrawingDialog {
    fn draw(&mut self, ui: &mut egui::Ui, host: &mut dyn katana_window::host::Host) {
        self.ui(ui, host);
    }
}

impl Drawn for katana_window::plotting::view_image_export::ViewImageDialog {
    fn draw(&mut self, ui: &mut egui::Ui, host: &mut dyn katana_window::host::Host) {
        self.ui(ui, host);
    }
}

/// A shown dialog, its host and the harness that draws them, its acts addressed to the dialog's
/// target: the C++ tests' dialog and its child widgets, found by their object names.
pub struct Bench<D: Drawn> {
    pub harness: katana_window::drive::Harness,
    pub dialog: D,
    pub host: katana_window::host::RecordingHost,
}

impl<D: Drawn> Bench<D> {
    /// `dialog`, already shown, drawn once in the target `target`.
    pub fn new(target: &str, dialog: D, host: katana_window::host::RecordingHost) -> Self {
        let mut harness = katana_window::drive::Harness::new();
        harness.target(target);
        let mut bench = Self {
            harness,
            dialog,
            host,
        };
        bench.draw();
        bench
    }

    /// Frames until the window is idle. An act refused fails the test with the C++'s sentence.
    pub fn draw(&mut self) {
        let dialog = &mut self.dialog;
        let host = &mut self.host;
        self.harness
            .run(|ui| dialog.draw(ui, &mut *host))
            .unwrap_or_else(|problem| panic!("{problem}"));
    }

    /// A field typed into, a choice made by its text, a number box set, a check box "on" or "off".
    pub fn fill(&mut self, name: &str, text: &str) {
        self.harness.fill(name, text);
        self.draw();
    }

    /// A click.
    pub fn press(&mut self, name: &str) {
        self.harness.press(name);
        self.draw();
    }

    /// The control `name` as the last frame drew it.
    pub fn control(&self, name: &str) -> katana_window::drive::ControlRecord {
        self.harness
            .control(name)
            .unwrap_or_else(|| panic!("no {name}"))
    }

    /// What a field holds (QLineEdit::text).
    pub fn value(&self, name: &str) -> String {
        self.control(name).value
    }

    /// What a label says (QLabel::text).
    pub fn text(&self, name: &str) -> String {
        self.control(name).text
    }

    pub fn enabled(&self, name: &str) -> bool {
        self.control(name).enabled
    }
}
