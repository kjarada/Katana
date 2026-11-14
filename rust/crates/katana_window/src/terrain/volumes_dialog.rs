//! Terrain > Volumes (terrain/volumes_dialog.hpp; docs/volumes.md, "In the window"): the earthworks
//! between the drawing's surfaces, a page per method - Surface to Surface, Surface to Level,
//! Stockpile, Along Alignment, Grid - and the cut/fill map.
//!
//! The dialog measures nothing itself (CLAUDE.md section 1): each page's fields make the VOLUME
//! line a person would type, shown in volumesCommand as it will run, and Measure hands that line to
//! the window's one executor (`Host::run`). The reply's records fill the results table, and on the
//! Along Alignment page the mass haul diagram; Copy puts the table on the clipboard, Save CSV runs
//! the same line with csv=, Make Cut/Fill Map runs SURFACE DIFFERENCE and Make Envelope SURFACE
//! ENVELOPE. So a person in the window, katana_cli and an agent through katana_mcp get the same
//! numbers from the same code, and the log shows every line.
//!
//! The boundary is the shared "Apply to" and "Only those that match" controls
//! (customisation::scope_filter_widget), or corners typed as a line takes them, or none (the whole
//! of the surfaces).
//!
//! ```text
//! volumesDialog
//! volumesMethods             the pages: volumesSurfacesPage, volumesLevelPage,
//!                            volumesStockpilePage, volumesSectionsPage, volumesGridPage
//! Surface to Surface:        volumesBase, volumesTop
//! Surface to Level:          volumesLevelSurface, volumesLevel
//! Stockpile:                 volumesStockpileSurface, volumesStockpileBase, volumesStockpileLevel
//! Along Alignment:           volumesSectionsBase, volumesSectionsTop, volumesAlignment,
//!                            volumesInterval, volumesFrom, volumesTo, volumesLeft, volumesRight,
//!                            volumesSectionMethod, volumesCurvature, volumesMassHaul
//! Grid:                      volumesGridBase, volumesGridTop, volumesCell
//! the boundary:              volumesBoundaryGroup, volumesBoundaryNone, volumesBoundaryScope,
//!                            volumesBoundaryCorners, volumesCorners, volumesEach, and the scope's
//!                            controls with the prefix volumes (volumesScope, volumesScopeDrawing,
//!                            volumesFilterLayer ...)
//! volumesBulk, volumesShrink
//! volumesCommand, volumesRun, volumesResults, volumesCopy
//! volumesCsvPath, volumesCsvBrowse, volumesSaveCsv
//! volumesMapName, volumesCutFillMap
//! volumesEnvelopeSide, volumesEnvelopeName, volumesEnvelope
//! volumesStatus, volumesReply, volumesClose
//! ```
//!
//! THE LISTS FOLLOW THE DRAWING. The dialog is non-modal and kept, so the drawing changes under
//! it: an undo that removes a surface it offers, a typed SURFACE line, New, Open. A DocumentWatcher
//! says so at the next frame, and the surfaces, alignments and layers are read again with what is
//! chosen kept where it is still there.
//!
//! The C++'s VolumesDialogContext - the executor, the drawing, the views, whether the session is
//! headless - is the `Host` the dialog is drawn with; showVolumesDialog, which found the kept
//! dialog among the window's children, is the window keeping this model between frames and calling
//! `show`.

use egui::{RichText, Ui};
use katana_core::error::make_error;
use katana_core::text::{format_exact_real, parse_finite_double, read_reply_record};
use katana_core::{ErrorCode, Result};

use crate::annotation::dialog_support::table;
use crate::canvas::{
    Brush, Canvas, HAlign, Image, Path, Pen, PenStyle, PointF, RasterCanvas, RectF, RenderHint,
    TextFlags, VAlign,
};
use crate::customisation::{DocumentWatcher, ScopeFilterWidget};
use crate::drive::controls::custom;
use crate::drive::registry::{ControlKind, ControlRecord};
use crate::drive::{
    Choice, LineOptions, SpinRange, TextOptions, button, check, choice, label_as, line_with,
    radio, spin, tabs, text_with,
};
use crate::geo::geo_dialog_support::line_word;
use crate::geo::terrain_dialog_parts::{Framed, WordChoice, host_views, row, show_framed};
use crate::host::{Host, VerbOutcome};
use crate::icons::{IconMode, show_icon};
use crate::ifc_dialogs::get_save_file_name;
use crate::shell::DialogFrame;
use crate::survey::survey_dialogs::offstage;
use crate::theme::palette;

/// A number as a line takes it back: the shortest text that reads as it.
fn number(value: f64) -> String {
    format_exact_real(value)
}

const NO_SURFACE: &str = "the drawing has no surface yet; make one with Terrain > Surface From";
const NO_ALIGNMENT: &str =
    "the drawing has no alignment yet; define one in Terrain > Alignment Manager";
const BASE_TIP: &str = "The existing surface: cut is where it is above the top";
const TOP_TIP: &str = "The design surface: fill is where it is above the base";
const REACH_TIP: &str = ". Blank reaches across all the ground the surfaces share, which is right \
     on a straight run only: on a curve or a corner give a reach under the curve's radius, because \
     sections that cross one another count ground twice and the line is refused; so are two \
     stretches of the alignment that run side by side, unless the reach is under half the distance \
     between them.";
const CSV_FILTER: &str = "CSV files (*.csv)";

// The number boxes, as numberBox made them: a QDoubleSpinBox's single step is 1 unless set.
const LEVEL: SpinRange = SpinRange::new(-100_000.0, 100_000.0, 1.0, 3).with_suffix(" m");
const INTERVAL: SpinRange = SpinRange::new(0.01, 100_000.0, 1.0, 3).with_suffix(" m");
const CELL: SpinRange = SpinRange::new(0.001, 10_000.0, 1.0, 3).with_suffix(" m");
const FACTOR: SpinRange = SpinRange::new(0.01, 10.0, 1.0, 3);

const PAGES: [&str; 5] = [
    "Surface to Surface",
    "Surface to Level",
    "Stockpile",
    "Along Alignment",
    "Grid",
];
const PAGE_NAMES: [&str; 5] = [
    "volumesSurfacesPage",
    "volumesLevelPage",
    "volumesStockpilePage",
    "volumesSectionsPage",
    "volumesGridPage",
];

/// The most of the results table shown before it scrolls, and the least it takes (its minimum
/// height in the C++).
const RESULTS_HEIGHT: f32 = 220.0;
const RESULTS_LEAST: f32 = 140.0;
/// The mass haul diagram's height: the C++ widget's minimum.
const CHART_HEIGHT: f32 = 140.0;

/// The surface (or alignment) a list has chosen, as one word of a line.
fn chosen(list: &Choice, items: &[String], field: &str, missing: &str) -> Result<String> {
    let text = list.current_text(items);
    if text.is_empty() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!("{field}: {missing}"),
            "",
        ));
    }
    line_word(&text, field)
}

/// Both of two words, or the first one's refusal (the C++'s `base ? top.error() : base.error()`).
fn both(first: Result<String>, second: Result<String>) -> Result<(String, String)> {
    let first = first?;
    Ok((first, second?))
}

/// `chosen` refilled from `old` to `names`, keeping the choice where it is still offered; a second
/// list of two (the top) starts on the second name, so the two do not begin as one surface
/// measured against itself - and a list refilled otherwise chooses its first name, as a QComboBox
/// given items does.
fn refill(list: &mut Choice, old: &[String], names: &[String], second: bool) {
    let kept = list.current_text(old);
    let otherwise = if second && names.len() > 1 {
        Some(1)
    } else if names.is_empty() {
        None
    } else {
        Some(0)
    };
    list.index = names.iter().position(|name| *name == kept).or(otherwise);
}

/// "x,y" or "x,y,z": a corner as a line types it.
fn is_corner(word: &str) -> bool {
    let parts: Vec<&str> = word.split(',').collect();
    if parts.len() != 2 && parts.len() != 3 {
        return false;
    }
    parts
        .iter()
        .all(|part| parse_finite_double(part.trim()).is_some())
}

/// key=value of a field left blank for the verb's default, or a number.
fn optional_number(text: &str, key: &str, field: &str) -> Result<Option<String>> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    let Some(value) = parse_finite_double(text) else {
        return Err(make_error(
            ErrorCode::ParseFailure,
            format!("{field} is a number, or blank for the default"),
            "",
        ));
    };
    Ok(Some(format!("{key}={}", number(value))))
}

/// QString::toDouble with its ok flag: the number a cell holds, None when it holds none.
fn cell_number(text: &str) -> Option<f64> {
    parse_finite_double(text.trim())
}

// ---- the results --------------------------------------------------------------------------------

/// The reply's records as a table: a column per key in the order the keys are first met, the
/// record's kind ("volume", "polygon", "section") first, and a row per record. What the results
/// table shows and Copy copies.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VolumeResultTable {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

impl VolumeResultTable {
    fn column(&self, key: &str) -> Option<usize> {
        self.columns.iter().position(|column| column == key)
    }
}

pub fn volume_result_table(reply: &str) -> VolumeResultTable {
    let mut table = VolumeResultTable {
        columns: vec![String::from("record")],
        rows: Vec::new(),
    };
    let mut records = Vec::new();
    for line in reply.split('\n') {
        let Some(record) = read_reply_record(line.trim()) else {
            continue;
        };
        if record.words.is_empty() && record.fields.is_empty() {
            continue;
        }
        for (key, _) in &record.fields {
            if !table.columns.contains(key) {
                table.columns.push(key.clone());
            }
        }
        records.push(record);
    }
    for record in &records {
        let mut row = vec![record.words.join(" ")];
        for column in &table.columns[1..] {
            row.push(String::from(record.value(column).unwrap_or("")));
        }
        table.rows.push(row);
    }
    table
}

// ---- the mass haul diagram ----------------------------------------------------------------------

/// The mass haul diagram: the cumulative cut less fill (the `mass` of each `section` record)
/// against station, the zero line drawn. Painted on the raster canvas when its points or its size
/// change, and kept as a texture.
#[derive(Default)]
pub struct MassHaulChart {
    points: Vec<PointF>,
    texture: Option<egui::TextureHandle>,
    /// The device pixels and ratio the texture was painted at.
    painted: Option<(u32, u32, u32)>,
}

impl MassHaulChart {
    /// (station, ordinate) pairs, ascending station; none clears it.
    pub fn set_points(&mut self, points: Vec<PointF>) {
        self.points = points;
        self.painted = None;
    }

    pub fn points(&self) -> &[PointF] {
        &self.points
    }

    /// The diagram painted on `canvas`, `width` by `height` of its logical units.
    #[allow(
        clippy::neg_cmp_op_on_partial_ord,
        reason = "The C++'s !(a > b) is kept: it rejects NaN, which a > b alone would let through."
    )]
    pub fn paint(&self, canvas: &mut dyn Canvas, width: f64, height: f64) {
        canvas.set_render_hint(RenderHint::Antialiasing, true);
        let whole = RectF::new(0.0, 0.0, width, height);
        // QPalette::Base: the panel colour (theme.cpp).
        canvas.fill_rect(whole, &Brush::from(palette::panel()));
        let frame = whole.adjusted(48.0, 10.0, -12.0, -22.0);
        canvas.set_pen(Pen::new(palette::text()));
        if self.points.len() < 2 || !(frame.width > 0.0) || !(frame.height > 0.0) {
            canvas.draw_text_in(
                whole,
                TextFlags::CENTER,
                "Measure along an alignment for its mass haul diagram",
            );
            return;
        }
        let mut low = 0.0_f64;
        let mut high = 0.0_f64;
        for point in &self.points {
            low = low.min(point.y);
            high = high.max(point.y);
        }
        if !(high > low) {
            high = low + 1.0; // all zero: a flat line on the axis
        }
        let first = self.points[0].x;
        let last = self.points[self.points.len() - 1].x.max(first + 1e-9);
        let place = |point: PointF| {
            PointF::new(
                frame.left() + (point.x - first) / (last - first) * frame.width,
                frame.bottom() - (point.y - low) / (high - low) * frame.height,
            )
        };
        // The zero line: where the haul balances.
        canvas.set_pen(Pen {
            color: palette::text(),
            width: 1.0,
            style: PenStyle::Dash,
            ..Pen::default()
        });
        let zero = place(PointF::new(first, 0.0)).y;
        canvas.draw_line(
            PointF::new(frame.left(), zero),
            PointF::new(frame.right(), zero),
        );
        let mut curve = Path::new();
        curve.move_to(place(self.points[0]));
        for point in &self.points[1..] {
            curve.line_to(place(*point));
        }
        canvas.stroke_path(
            &curve,
            &Pen {
                color: palette::accent(),
                width: 2.0,
                ..Pen::default()
            },
        );
        canvas.set_pen(Pen::new(palette::text()));
        let label = |value: f64| crate::format::qt_fixed(value, 0);
        let right = TextFlags::aligned(HAlign::Right, VAlign::Top);
        let left = TextFlags::aligned(HAlign::Left, VAlign::Top);
        canvas.draw_text_in(
            RectF::new(0.0, frame.top() - 6.0, 44.0, 14.0),
            right,
            &label(high),
        );
        canvas.draw_text_in(
            RectF::new(0.0, frame.bottom() - 8.0, 44.0, 14.0),
            right,
            &label(low),
        );
        canvas.draw_text_in(
            RectF::new(frame.left(), frame.bottom() + 4.0, 80.0, 16.0),
            left,
            &label(first),
        );
        canvas.draw_text_in(
            RectF::new(frame.right() - 80.0, frame.bottom() + 4.0, 80.0, 16.0),
            right,
            &label(last),
        );
    }

    /// The diagram as an image of `width` by `height` points at `ratio` device pixels a point.
    pub fn image(&self, width: u32, height: u32, ratio: f64) -> Image {
        let pixels = |points: u32| (f64::from(points) * ratio).round().max(1.0) as u32;
        let mut image = Image::new(pixels(width), pixels(height));
        image.set_device_pixel_ratio(ratio);
        let mut canvas = RasterCanvas::on(image);
        self.paint(&mut canvas, f64::from(width), f64::from(height));
        canvas.into_image()
    }

    /// The diagram across the width there is, named volumesMassHaul.
    fn ui(&mut self, ui: &mut Ui) {
        let width = ui.available_width().max(80.0);
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(width, CHART_HEIGHT), egui::Sense::hover());
        if ui.is_rect_visible(rect) {
            let ratio = ui.ctx().pixels_per_point();
            let size = (
                rect.width().round().max(1.0) as u32,
                rect.height().round().max(1.0) as u32,
                ratio.to_bits(),
            );
            if self.texture.is_none() || self.painted != Some(size) {
                let image = self.image(size.0, size.1, f64::from(ratio));
                self.texture = Some(ui.ctx().load_texture(
                    "volumesMassHaul",
                    image.to_color_image(),
                    egui::TextureOptions::LINEAR,
                ));
                self.painted = Some(size);
            }
            if let Some(texture) = &self.texture {
                ui.painter().image(
                    texture.id(),
                    rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }
        }
        let response = response.on_hover_text(
            "The mass haul diagram: cut less fill summed from the start of the range, against \
             station. Rising: a surplus of cut; falling: fill to be brought.",
        );
        custom(
            ui,
            &response,
            ControlRecord::new("volumesMassHaul", ControlKind::Other),
        );
    }
}

// ---- the dialog ---------------------------------------------------------------------------------

/// Which page is showing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum VolumeMethod {
    #[default]
    Surfaces,
    Level,
    Stockpile,
    Sections,
    Grid,
}

const METHODS: [VolumeMethod; 5] = [
    VolumeMethod::Surfaces,
    VolumeMethod::Level,
    VolumeMethod::Stockpile,
    VolumeMethod::Sections,
    VolumeMethod::Grid,
];

/// What the boundary is: one of three radio buttons.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Boundary {
    /// Everywhere: all the ground the surfaces have in common.
    #[default]
    None,
    /// The polygons the scope takes.
    Scope,
    /// One polygon, its corners typed.
    Corners,
}

/// What a frame's clicks ask for, done after the dialog's body is drawn.
#[derive(Clone, Copy)]
enum Asked {
    Measure,
    Copy,
    Browse,
    SaveCsv,
    CutFillMap,
    Envelope,
}

/// Terrain > Volumes (see the module comment).
pub struct VolumesDialog {
    // First, so it goes first: no delivery reaches a half-dropped dialog.
    watcher: DocumentWatcher,
    frame: DialogFrame,
    visible: bool,
    page: usize,
    /// The drawing's surfaces and alignments, every list of them offering these.
    surfaces: Vec<String>,
    alignments: Vec<String>,
    // Surface to Surface
    base: Choice,
    top: Choice,
    // Surface to Level
    level_surface: Choice,
    level: f64,
    // Stockpile
    stockpile_surface: Choice,
    stockpile_base: WordChoice,
    stockpile_level: f64,
    // Along Alignment
    sections_base: Choice,
    sections_top: Choice,
    alignment: Choice,
    interval: f64,
    from: String,
    to: String,
    left: String,
    right: String,
    section_method: WordChoice,
    curvature: bool,
    chart: MassHaulChart,
    // Grid
    grid_base: Choice,
    grid_top: Choice,
    cell: f64,
    // The boundary
    boundary: Boundary,
    corners: String,
    each: bool,
    scope: ScopeFilterWidget,
    // Factors
    bulk: f64,
    shrink: f64,
    // The results and what happened
    table: VolumeResultTable,
    csv_path: String,
    map_name: String,
    envelope_side: WordChoice,
    envelope_name: String,
    status: String,
    status_is_error: bool,
    reply: String,
}

impl VolumesDialog {
    /// The dialog over the window's drawing and views, its lists read.
    pub fn new(host: &mut dyn Host) -> Self {
        let mut scope = ScopeFilterWidget::new("volumes");
        scope.views = host_views(host);
        let mut dialog = Self {
            watcher: DocumentWatcher::new(host.document()),
            frame: DialogFrame::new("volumesDialog", "Volumes"),
            visible: false,
            page: 0,
            surfaces: Vec::new(),
            alignments: Vec::new(),
            base: Choice::default(),
            top: Choice::default(),
            level_surface: Choice::default(),
            level: 0.0,
            stockpile_surface: Choice::default(),
            stockpile_base: WordChoice::new(&[
                ("The boundary's heights", "boundary"),
                ("The lowest point on the boundary", "lowest"),
                ("A level", "level"),
            ]),
            stockpile_level: 0.0,
            sections_base: Choice::default(),
            sections_top: Choice::default(),
            alignment: Choice::default(),
            interval: 10.0,
            from: String::new(),
            to: String::new(),
            left: String::new(),
            right: String::new(),
            section_method: WordChoice::new(&[
                ("Average end area", "endarea"),
                ("Prismoidal", "prismoidal"),
                ("Both, with the prismoidal correction", "both"),
            ]),
            // The verb's default: the correction made.
            curvature: true,
            chart: MassHaulChart::default(),
            grid_base: Choice::default(),
            grid_top: Choice::default(),
            cell: 1.0,
            boundary: Boundary::None,
            corners: String::new(),
            each: false,
            scope,
            bulk: 1.0,
            shrink: 1.0,
            table: VolumeResultTable::default(),
            csv_path: String::new(),
            map_name: String::new(),
            envelope_side: WordChoice::new(&[
                ("The higher of the two", "MAX"),
                ("The lower of the two", "MIN"),
            ]),
            envelope_name: String::new(),
            status: String::new(),
            status_is_error: false,
            reply: String::new(),
        };
        dialog.reload(&*host);
        dialog
    }

    /// The dialog's object name.
    pub fn object_name(&self) -> &str {
        self.frame.name()
    }

    /// Terrain > Volumes: the lists read again and the dialog shown (showVolumesDialog). The
    /// window keeps the dialog between uses, so its fields survive.
    pub fn show(&mut self, host: &dyn Host) {
        self.reload(host);
        self.visible = true;
    }

    pub fn hide(&mut self) {
        self.visible = false;
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// The surfaces, alignments, layers and views read again, keeping what is chosen where it is
    /// still there: when the dialog is shown, after a cut/fill map or an envelope is made, and
    /// whenever the drawing changes.
    pub fn reload(&mut self, host: &dyn Host) {
        let document = host.document();
        let surfaces: Vec<String> = document
            .surfaces()
            .all()
            .iter()
            .map(|surface| surface.name.clone())
            .collect();
        let alignments = document.model().alignments.names();
        self.scope.reload(document.model());
        let old = std::mem::replace(&mut self.surfaces, surfaces);
        for list in [
            &mut self.base,
            &mut self.level_surface,
            &mut self.stockpile_surface,
            &mut self.sections_base,
            &mut self.grid_base,
        ] {
            refill(list, &old, &self.surfaces, false);
        }
        for list in [&mut self.top, &mut self.sections_top, &mut self.grid_top] {
            refill(list, &old, &self.surfaces, true);
        }
        let old = std::mem::replace(&mut self.alignments, alignments);
        refill(&mut self.alignment, &old, &self.alignments, false);
    }

    pub fn set_method(&mut self, method: VolumeMethod) {
        self.page = METHODS
            .iter()
            .position(|listed| *listed == method)
            .unwrap_or(0);
    }

    pub fn method(&self) -> VolumeMethod {
        METHODS[self.page.min(METHODS.len() - 1)]
    }

    /// The scope and filter controls of the boundary's polygons.
    pub fn scope_controls(&mut self) -> &mut ScopeFilterWidget {
        &mut self.scope
    }

    pub fn chart(&self) -> &MassHaulChart {
        &self.chart
    }

    /// What the last line did, or why none ran.
    pub fn status(&self) -> &str {
        &self.status
    }

    /// The last reply, or the refusal.
    pub fn reply(&self) -> &str {
        &self.reply
    }

    fn boundary_words(&self, required: bool, each: bool) -> Result<String> {
        let mut words = match self.boundary {
            Boundary::None => {
                if required {
                    return Err(make_error(
                        ErrorCode::InvalidArgument,
                        "Inside: a stockpile is measured inside its boundary; choose the polygons \
                         or type its corners",
                        "",
                    ));
                }
                return Ok(String::new());
            }
            Boundary::Scope => format!("INSIDE FROM {}", self.scope.verb_words()?),
            Boundary::Corners => {
                // QString::split(' ', SkipEmptyParts): blanks only, runs of them one.
                let corners: Vec<&str> = self
                    .corners
                    .split(' ')
                    .filter(|word| !word.is_empty())
                    .collect();
                if corners.len() < 3 || !corners.iter().all(|corner| is_corner(corner)) {
                    return Err(make_error(
                        ErrorCode::ParseFailure,
                        "Corners: three or more x,y or x,y,z, separated by blanks",
                        "",
                    ));
                }
                format!("INSIDE {}", corners.join(" "))
            }
        };
        if each && self.each {
            words.push_str(" EACH");
        }
        Ok(words)
    }

    fn factor_words(&self) -> String {
        // A factor of 1 changes nothing, so it is left off the line.
        let mut words = String::new();
        if self.bulk != 1.0 {
            words.push_str(" bulk=");
            words.push_str(&number(self.bulk));
        }
        if self.shrink != 1.0 {
            words.push_str(" shrink=");
            words.push_str(&number(self.shrink));
        }
        words
    }

    /// The VOLUME line the showing page's fields say, or why there is none, naming the field.
    pub fn line(&self) -> Result<String> {
        let surfaces = &self.surfaces;
        let mut words = vec![String::from("VOLUME")];
        let mut inside = None;
        match self.method() {
            VolumeMethod::Surfaces => {
                let (base, top) = both(
                    chosen(&self.base, surfaces, "Base", NO_SURFACE),
                    chosen(&self.top, surfaces, "Top", NO_SURFACE),
                )?;
                words.extend([String::from("SURFACES"), base, top]);
                inside = Some(self.boundary_words(false, true));
            }
            VolumeMethod::Level => {
                let surface = chosen(&self.level_surface, surfaces, "Surface", NO_SURFACE)?;
                words.extend([String::from("DATUM"), surface, number(self.level)]);
                inside = Some(self.boundary_words(false, true));
            }
            VolumeMethod::Stockpile => {
                let surface = chosen(&self.stockpile_surface, surfaces, "Surface", NO_SURFACE)?;
                words.extend([String::from("STOCKPILE"), surface]);
                inside = Some(self.boundary_words(true, true));
            }
            VolumeMethod::Sections => {
                let (base, top) = both(
                    chosen(&self.sections_base, surfaces, "Base", NO_SURFACE),
                    chosen(&self.sections_top, surfaces, "Top", NO_SURFACE),
                )?;
                let alignment = chosen(&self.alignment, &self.alignments, "Alignment", NO_ALIGNMENT)?;
                words.extend([
                    String::from("SECTIONS"),
                    base,
                    top,
                    String::from("ALIGNMENT"),
                    alignment,
                    format!("interval={}", number(self.interval)),
                ]);
                for (text, key, field) in [
                    (&self.from, "from", "From"),
                    (&self.to, "to", "To"),
                    (&self.left, "left", "Left"),
                    (&self.right, "right", "Right"),
                ] {
                    if let Some(word) = optional_number(text, key, field)? {
                        words.push(word);
                    }
                }
                let method = self.section_method.word();
                if method != "endarea" {
                    words.push(format!("method={method}"));
                }
                // On is the verb's default, so only the plain figure is a word.
                if !self.curvature {
                    words.push(String::from("curvature=off"));
                }
            }
            VolumeMethod::Grid => {
                let (base, top) = both(
                    chosen(&self.grid_base, surfaces, "Base", NO_SURFACE),
                    chosen(&self.grid_top, surfaces, "Top", NO_SURFACE),
                )?;
                words.extend([
                    String::from("GRID"),
                    base,
                    top,
                    format!("cell={}", number(self.cell)),
                ]);
                inside = Some(self.boundary_words(false, false));
            }
        }
        if let Some(inside) = inside {
            let inside = inside?;
            if !inside.is_empty() {
                words.push(inside);
            }
        }
        if self.method() == VolumeMethod::Stockpile {
            match self.stockpile_base.word() {
                "lowest" => words.push(String::from("base=lowest")),
                "level" => words.push(format!("level={}", number(self.stockpile_level))),
                _ => {}
            }
        }
        Ok(words.join(" ") + &self.factor_words())
    }

    /// The two surfaces the showing page has chosen (the Surface to Surface page's when the page
    /// has one surface), as words of a line.
    fn surface_pair(&self) -> Result<(String, String)> {
        let (base, top) = match self.method() {
            VolumeMethod::Sections => (&self.sections_base, &self.sections_top),
            VolumeMethod::Grid => (&self.grid_base, &self.grid_top),
            _ => (&self.base, &self.top),
        };
        both(
            chosen(base, &self.surfaces, "Base", NO_SURFACE),
            chosen(top, &self.surfaces, "Top", NO_SURFACE),
        )
    }

    /// SURFACE DIFFERENCE of the page's two surfaces, with the map's name when given.
    pub fn cut_fill_map_line(&self) -> Result<String> {
        let (base, top) = self.surface_pair()?;
        let mut text = format!("SURFACE DIFFERENCE {base} {top}");
        let name = self.map_name.trim();
        if !name.is_empty() {
            text.push_str(" NAME ");
            text.push_str(&line_word(name, "Map name")?);
        }
        Ok(text)
    }

    /// SURFACE ENVELOPE of the same two surfaces: the higher (MAX) or the lower (MIN) of them, as
    /// volumesEnvelopeSide says, with the name when given.
    pub fn envelope_line(&self) -> Result<String> {
        let (base, top) = self.surface_pair()?;
        let mut text = format!(
            "SURFACE ENVELOPE {base} {top} {}",
            self.envelope_side.word()
        );
        let name = self.envelope_name.trim();
        if !name.is_empty() {
            text.push_str(" NAME ");
            text.push_str(&line_word(name, "Envelope name")?);
        }
        Ok(text)
    }

    fn set_status(&mut self, text: impl Into<String>, is_error: bool) {
        self.status = text.into();
        self.status_is_error = is_error;
    }

    fn show_reply(&mut self, outcome: &VerbOutcome) {
        let text = if outcome.ok {
            outcome.reply.clone()
        } else if outcome.reply.is_empty() {
            outcome.error.clone()
        } else {
            format!("{}\n{}", outcome.reply, outcome.error)
        };
        self.reply = String::from(text.trim());
        if !outcome.ok {
            let error = outcome.error.trim();
            let said = if error.is_empty() {
                "The line was refused."
            } else {
                error
            };
            self.set_status(said, true);
            return;
        }
        self.table = volume_result_table(&outcome.reply);
        let station_column = self.table.column("station").filter(|at| *at > 0);
        let mass_column = self.table.column("mass").filter(|at| *at > 0);
        let mut haul = Vec::new();
        for row in &self.table.rows {
            // The first section has no volume before it, so no ordinate: the diagram starts at 0
            // there.
            if row[0] != "section" {
                continue;
            }
            let Some(station_column) = station_column else {
                continue;
            };
            if let Some(station) = cell_number(&row[station_column]) {
                let mass = mass_column
                    .and_then(|at| cell_number(&row[at]))
                    .unwrap_or(0.0);
                haul.push(PointF::new(station, mass));
            }
        }
        self.chart.set_points(haul);
        // The totals, said as a sentence from the last volume record.
        let mut said = String::from("Measured.");
        if let Some(row) = self.table.rows.iter().rev().find(|row| row[0] == "volume") {
            let field = |key: &str| {
                self.table
                    .column(key)
                    .filter(|at| *at > 0 && *at < row.len())
                    .map(|at| row[at].clone())
                    .unwrap_or_default()
            };
            if !field("cut").is_empty() {
                said = format!(
                    "Cut {} m3, fill {} m3, net {} m3 (fill less cut)",
                    field("cut"),
                    field("fill"),
                    field("net")
                );
                let method = field("method");
                if !method.is_empty() {
                    said.push_str(&format!(", by the {method} method"));
                }
                said.push('.');
            } else if field("ran") == "no" {
                said = String::from("Nothing measured: the scope took no closed polygon.");
            }
        }
        self.set_status(said, false);
    }

    /// Measure: the line run, its records put in the table and, along an alignment, the mass haul
    /// diagram. False when there was no line or the line was refused, with the reason in the
    /// status.
    pub fn measure(&mut self, host: &mut dyn Host) -> bool {
        let made = match self.line() {
            Ok(made) => made,
            Err(error) => {
                self.set_status(error.message, true);
                return false;
            }
        };
        let outcome = host.run(&made);
        self.show_reply(&outcome);
        outcome.ok
    }

    /// The line run again with csv= the file field names, replacing it (a file dialog asked first
    /// when it chose the file). True when the file was written: not when the line was refused, nor
    /// when nothing was measured (a scope that takes no polygon), which the status says.
    pub fn save_csv(&mut self, host: &mut dyn Host) -> bool {
        let mut path = String::from(self.csv_path.trim());
        if path.is_empty() && !host.headless() {
            path = get_save_file_name("Save Volumes as CSV", "", CSV_FILTER);
            self.csv_path.clone_from(&path);
        }
        if path.is_empty() {
            self.set_status(
                "Give the file to save the table to in the CSV field.",
                true,
            );
            return false;
        }
        let made = match self.line() {
            Ok(made) => made,
            Err(error) => {
                self.set_status(error.message, true);
                return false;
            }
        };
        let file = match line_word(&format!("csv={path}"), "CSV file") {
            Ok(file) => file,
            Err(error) => {
                self.set_status(error.message, true);
                return false;
            }
        };
        // Save replaces the file it names; the file dialog asked before choosing one.
        let outcome = host.run(&format!("{made} {file} OVERWRITE"));
        self.show_reply(&outcome);
        if !outcome.ok {
            return false;
        }
        // Saved only when the reply says it wrote the file (a `csv` record with its rows): a scope
        // that takes no polygon runs nothing and writes nothing, so a file an earlier save left is
        // as it was and must not be called saved.
        let wrote = self
            .table
            .column("rows")
            .filter(|at| *at > 0)
            .is_some_and(|at| {
                self.table
                    .rows
                    .iter()
                    .any(|row| row[0] == "csv" && at < row.len() && !row[at].is_empty())
            });
        if wrote {
            self.set_status(format!("Saved the table to {path}."), false);
        } else {
            self.set_status(
                format!("Nothing was measured, so nothing was saved: {path} was not written."),
                false,
            );
        }
        wrote
    }

    /// A line that makes a surface run through the executor, reported and the lists reloaded so
    /// the new surface is offered at once.
    fn run_making(&mut self, host: &mut dyn Host, line: Result<String>, done: &str) -> bool {
        let line = match line {
            Ok(line) => line,
            Err(error) => {
                self.set_status(error.message, true);
                return false;
            }
        };
        let outcome = host.run(&line);
        let said = if outcome.ok {
            &outcome.reply
        } else {
            &outcome.error
        };
        self.reply = String::from(said.trim());
        if !outcome.ok {
            self.set_status(outcome.error.trim(), true);
            return false;
        }
        self.set_status(done, false);
        self.reload(&*host);
        true
    }

    /// SURFACE DIFFERENCE run, and the surface lists reloaded so the map is offered at once.
    pub fn make_cut_fill_map(&mut self, host: &mut dyn Host) -> bool {
        let line = self.cut_fill_map_line();
        self.run_making(
            host,
            line,
            "Made the cut/fill map: a new surface in the Cut Fill style, fill above zero and cut \
             below. UNDO removes it.",
        )
    }

    /// SURFACE ENVELOPE run, and the surface lists reloaded the same way.
    pub fn make_envelope(&mut self, host: &mut dyn Host) -> bool {
        let line = self.envelope_line();
        let side = if self.envelope_side.word() == "MAX" {
            "higher"
        } else {
            "lower"
        };
        let done = format!(
            "Made the envelope: a new surface following the {side} of the base and the top, in \
             the default style. UNDO removes it."
        );
        self.run_making(host, line, &done)
    }

    /// The table as tab-separated text with its header: what Copy puts on the clipboard.
    pub fn results_text(&self) -> String {
        let mut lines = vec![self.table.columns.join("\t")];
        for row in &self.table.rows {
            lines.push(row.join("\t"));
        }
        lines.join("\n")
    }

    fn browse(&mut self, host: &mut dyn Host) {
        if host.headless() {
            self.set_status(
                "Type the file in the CSV field: a headless run opens no file dialog.",
                true,
            );
            return;
        }
        let file = get_save_file_name("Save Volumes as CSV", &self.csv_path, CSV_FILTER);
        if !file.is_empty() {
            self.csv_path = file;
        }
    }

    fn copy(&mut self, host: &mut dyn Host) {
        let text = self.results_text();
        // QClipboard::setText cannot fail; a clipboard here can, and says why.
        if let Err(error) = host.clipboard().set_text(&text) {
            self.set_status(error.describe(), true);
        }
    }

    /// One frame of the dialog, when it is shown: its drawing's changes delivered first.
    pub fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        if !self.visible {
            return;
        }
        if self
            .watcher
            .deliver(host.document())
            .is_some_and(|changes| changes.model)
        {
            self.reload(&*host);
        }
        if show_framed(self, ui, host) {
            self.hide();
        }
    }

    // ---- what is drawn ----

    fn surfaces_page(&mut self, ui: &mut Ui) {
        let surfaces = &self.surfaces;
        row(ui, "Base:", |ui| {
            choice(ui, "volumesBase", surfaces, &mut self.base).on_hover_text(BASE_TIP);
        });
        row(ui, "Top:", |ui| {
            choice(ui, "volumesTop", surfaces, &mut self.top).on_hover_text(TOP_TIP);
        });
    }

    fn level_page(&mut self, ui: &mut Ui) {
        let surfaces = &self.surfaces;
        row(ui, "Surface:", |ui| {
            choice(ui, "volumesLevelSurface", surfaces, &mut self.level_surface)
                .on_hover_text("The surface measured against the level");
        });
        row(ui, "Level:", |ui| {
            spin(ui, "volumesLevel", &mut self.level, LEVEL).on_hover_text(
                "The level: what stands above it is cut, what lies below it is fill",
            );
        });
    }

    fn stockpile_page(&mut self, ui: &mut Ui) {
        let surfaces = &self.surfaces;
        row(ui, "Surface:", |ui| {
            choice(
                ui,
                "volumesStockpileSurface",
                surfaces,
                &mut self.stockpile_surface,
            )
            .on_hover_text("The surface the pile (or pit) is part of");
        });
        row(ui, "Base:", |ui| {
            self.stockpile_base
                .ui(ui, "volumesStockpileBase")
                .on_hover_text(
                    "What the pile is measured above: its toe's own heights (or the surface's on \
                     it), a level plane at the lowest point of the surface along the boundary, or \
                     a level",
                );
        });
        let levelled = self.stockpile_base.word() == "level";
        row(ui, "Level:", |ui| {
            ui.add_enabled_ui(levelled, |ui| {
                spin(
                    ui,
                    "volumesStockpileLevel",
                    &mut self.stockpile_level,
                    LEVEL,
                )
                .on_hover_text("The base's level");
            });
        });
        ui.label("The boundary below is the pile's toe: a polygon is required.");
    }

    fn sections_page(&mut self, ui: &mut Ui) {
        let surfaces = &self.surfaces;
        row(ui, "Base:", |ui| {
            choice(ui, "volumesSectionsBase", surfaces, &mut self.sections_base)
                .on_hover_text(BASE_TIP);
        });
        row(ui, "Top:", |ui| {
            choice(ui, "volumesSectionsTop", surfaces, &mut self.sections_top)
                .on_hover_text(TOP_TIP);
        });
        row(ui, "Alignment:", |ui| {
            choice(ui, "volumesAlignment", &self.alignments, &mut self.alignment)
                .on_hover_text("The alignment the sections are cut square to");
        });
        row(ui, "Interval:", |ui| {
            spin(ui, "volumesInterval", &mut self.interval, INTERVAL).on_hover_text(
                "Sections at every whole multiple of this station, and at the alignment's key \
                 stations",
            );
        });
        let station = |ui: &mut Ui, name: &str, value: &mut String, placeholder: &str, tip: &str| {
            line_with(
                ui,
                name,
                value,
                LineOptions {
                    placeholder,
                    ..LineOptions::default()
                },
            )
            .on_hover_text(tip);
        };
        row(ui, "From station:", |ui| {
            station(
                ui,
                "volumesFrom",
                &mut self.from,
                "the start",
                "The first station; blank for the alignment's start",
            );
        });
        row(ui, "To station:", |ui| {
            station(
                ui,
                "volumesTo",
                &mut self.to,
                "the end",
                "The last station; blank for the alignment's end",
            );
        });
        let left_tip =
            format!("How far each section reaches left of the centreline{REACH_TIP}");
        let right_tip =
            format!("How far each section reaches right of the centreline{REACH_TIP}");
        row(ui, "Left:", |ui| {
            station(
                ui,
                "volumesLeft",
                &mut self.left,
                "across the common ground",
                &left_tip,
            );
        });
        row(ui, "Right:", |ui| {
            station(
                ui,
                "volumesRight",
                &mut self.right,
                "across the common ground",
                &right_tip,
            );
        });
        row(ui, "Method:", |ui| {
            self.section_method
                .ui(ui, "volumesSectionMethod")
                .on_hover_text(
                    "Average end area: the mean of two sections' areas times the distance between \
                     them. Prismoidal: with a section cut halfway, exact where the area changes as \
                     a cubic",
                );
        });
        check(
            ui,
            "volumesCurvature",
            "Correct for curvature",
            &mut self.curvature,
        )
        .on_hover_text(
            "On a curve the ground at an offset from the centreline travels a different distance \
             than the centreline: less on the inside, more on the outside (Pappus). Checked, each \
             section's cut and fill are corrected for the offset of their centroids; unchecked, \
             the areas are taken along the centreline. A straight, and a section symmetric about \
             the centreline, are the same either way.",
        );
        self.chart.ui(ui);
    }

    fn grid_page(&mut self, ui: &mut Ui) {
        let surfaces = &self.surfaces;
        row(ui, "Base:", |ui| {
            choice(ui, "volumesGridBase", surfaces, &mut self.grid_base).on_hover_text(BASE_TIP);
        });
        row(ui, "Top:", |ui| {
            choice(ui, "volumesGridTop", surfaces, &mut self.grid_top).on_hover_text(TOP_TIP);
        });
        row(ui, "Cell:", |ui| {
            spin(ui, "volumesCell", &mut self.cell, CELL)
                .on_hover_text("The side of a cell; each is counted at its centre");
        });
        ui.label("A cross-check of the exact methods: the midpoint rule on square cells.");
    }

    /// The tabs and the page in front; the others are drawn offstage, as a QTabWidget's other
    /// pages are there to be filled. A page not in front takes no room, so the tabs are as tall
    /// as the page shown (the C++'s fitMethodPage).
    fn pages(&mut self, ui: &mut Ui) {
        tabs(ui, "volumesMethods", &PAGES, &mut self.page);
        let front = self.page;
        for (at, name) in PAGE_NAMES.iter().enumerate() {
            let draw = |ui: &mut Ui| {
                let page = ui.vertical(|ui| match at {
                    0 => self.surfaces_page(ui),
                    1 => self.level_page(ui),
                    2 => self.stockpile_page(ui),
                    3 => self.sections_page(ui),
                    _ => self.grid_page(ui),
                });
                custom(
                    ui,
                    &page.response,
                    ControlRecord::new(name, ControlKind::Other),
                );
            };
            if at == front {
                draw(ui);
            } else {
                offstage(ui, name, draw);
            }
        }
    }

    /// The boundary: none, the shared scope and filter, or typed corners.
    fn boundary_group(&mut self, ui: &mut Ui) {
        let showing = self.method();
        let group = ui
            .add_enabled_ui(showing != VolumeMethod::Sections, |ui| {
                ui.group(|ui| {
                    ui.label("Inside");
                    ui.horizontal(|ui| {
                        for (name, text, tip, which) in [
                            (
                                "volumesBoundaryNone",
                                "Everywhere",
                                "All the ground the surfaces have in common",
                                Boundary::None,
                            ),
                            (
                                "volumesBoundaryScope",
                                "The polygons below",
                                "The closed polylines, curve polylines, circles and ellipses the \
                                 scope takes; lines and points are counted, not used",
                                Boundary::Scope,
                            ),
                            (
                                "volumesBoundaryCorners",
                                "Typed corners",
                                "One polygon, its corners typed",
                                Boundary::Corners,
                            ),
                        ] {
                            if radio(ui, name, text, self.boundary == which)
                                .on_hover_text(tip)
                                .clicked()
                            {
                                self.boundary = which;
                            }
                        }
                        let polygons = self.boundary != Boundary::None;
                        ui.add_enabled_ui(polygons && showing != VolumeMethod::Grid, |ui| {
                            check(
                                ui,
                                "volumesEach",
                                "A record for each polygon",
                                &mut self.each,
                            )
                            .on_hover_text(
                                "Each polygon measured and reported on its own as well as in the \
                                 total; without it, polygons that overlap are refused",
                            );
                        });
                    });
                    ui.add_enabled_ui(self.boundary == Boundary::Corners, |ui| {
                        line_with(
                            ui,
                            "volumesCorners",
                            &mut self.corners,
                            LineOptions {
                                placeholder: "x,y x,y x,y ...",
                                ..LineOptions::default()
                            },
                        )
                        .on_hover_text("Three or more corners, x,y or x,y,z, separated by blanks");
                    });
                    ui.add_enabled_ui(self.boundary == Boundary::Scope, |ui| {
                        self.scope.ui(ui);
                    });
                })
                .response
            })
            .inner;
        custom(
            ui,
            &group,
            ControlRecord::new("volumesBoundaryGroup", ControlKind::Other),
        );
    }

    /// The factors, applied in the reply only.
    fn factors(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label("Bulk (cut x):");
            spin(ui, "volumesBulk", &mut self.bulk, FACTOR).on_hover_text(
                "Multiplies the cut in the reply (bank to loose, say); never the surfaces",
            );
            ui.label("Shrink (fill x):");
            spin(ui, "volumesShrink", &mut self.shrink, FACTOR).on_hover_text(
                "Multiplies the fill in the reply (compacted to bank, say); never the surfaces",
            );
        });
    }

    /// The line as it will run, and Measure.
    fn command_row(&mut self, ui: &mut Ui, asked: &mut Option<Asked>) {
        let made = self.line();
        let (mut shown, tip) = match &made {
            Ok(line) => (line.clone(), String::from("The line Measure runs")),
            Err(error) => (String::new(), error.message.clone()),
        };
        ui.horizontal(|ui| {
            line_with(
                ui,
                "volumesCommand",
                &mut shown,
                LineOptions {
                    read_only: true,
                    ..LineOptions::default()
                },
            )
            .on_hover_text(tip);
            if button(ui, "volumesRun", "Measure").clicked() {
                *asked = Some(Asked::Measure);
            }
        });
    }

    /// The results table: the reply's records, a column a key.
    fn results(&mut self, ui: &mut Ui) {
        let headers: Vec<&str> = self.table.columns.iter().map(String::as_str).collect();
        let rows = &self.table.rows;
        egui::ScrollArea::both()
            .id_salt("volumesResultsArea")
            .max_height(RESULTS_HEIGHT)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.set_min_height(RESULTS_LEAST);
                table(ui, "volumesResults", &headers, rows, None, &[], None);
            });
    }

    fn file_row(&mut self, ui: &mut Ui, asked: &mut Option<Asked>) {
        ui.horizontal(|ui| {
            if button(ui, "volumesCopy", "Copy")
                .on_hover_text("The table on the clipboard, tab-separated with its header")
                .clicked()
            {
                *asked = Some(Asked::Copy);
            }
            line_with(
                ui,
                "volumesCsvPath",
                &mut self.csv_path,
                LineOptions {
                    placeholder: "file.csv",
                    ..LineOptions::default()
                },
            )
            .on_hover_text("The file Save CSV writes; it is replaced");
            if button(ui, "volumesCsvBrowse", "Browse...").clicked() {
                *asked = Some(Asked::Browse);
            }
            if button(ui, "volumesSaveCsv", "Save CSV")
                .on_hover_text(
                    "The line run again with csv= the file: the quantities as a table, with the \
                     method and its inputs on every row. Nothing is written when nothing is \
                     measured.",
                )
                .clicked()
            {
                *asked = Some(Asked::SaveCsv);
            }
        });
    }

    fn map_row(&mut self, ui: &mut Ui, asked: &mut Option<Asked>) {
        ui.horizontal(|ui| {
            ui.label("Map name:");
            line_with(
                ui,
                "volumesMapName",
                &mut self.map_name,
                LineOptions {
                    placeholder: "top-base",
                    ..LineOptions::default()
                },
            )
            .on_hover_text("The cut/fill map's name; blank for top-base");
            // The button's icon, painted beside it: the drive's buttons carry text only.
            let (icon, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
            show_icon(ui.painter(), icon, "CutFillMap", IconMode::Normal);
            if button(ui, "volumesCutFillMap", "Make Cut/Fill Map")
                .on_hover_text(
                    "The top minus the base as a new surface in the Cut Fill style: SURFACE \
                     DIFFERENCE",
                )
                .clicked()
            {
                *asked = Some(Asked::CutFillMap);
            }
        });
    }

    fn envelope_row(&mut self, ui: &mut Ui, asked: &mut Option<Asked>) {
        ui.horizontal(|ui| {
            self.envelope_side
                .ui(ui, "volumesEnvelopeSide")
                .on_hover_text(
                    "Which of the two surfaces the envelope follows where they differ: the higher \
                     (the finished ground over an as-built, say) or the lower",
                );
            ui.label("Envelope name:");
            line_with(
                ui,
                "volumesEnvelopeName",
                &mut self.envelope_name,
                LineOptions {
                    placeholder: "base-top-max",
                    ..LineOptions::default()
                },
            )
            .on_hover_text("The envelope's name; blank for base-top-max or base-top-min");
            if button(ui, "volumesEnvelope", "Make Envelope")
                .on_hover_text(
                    "The higher or lower of the base and the top as a new surface in the \
                     drawing's default style (real levels, not a difference): SURFACE ENVELOPE",
                )
                .clicked()
            {
                *asked = Some(Asked::Envelope);
            }
        });
    }
}

impl Framed for VolumesDialog {
    fn frame_mut(&mut self) -> &mut DialogFrame {
        &mut self.frame
    }

    fn body(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        let mut asked = None;
        self.pages(ui);
        self.boundary_group(ui);
        self.factors(ui);
        self.command_row(ui, &mut asked);
        self.results(ui);
        self.file_row(ui, &mut asked);
        self.map_row(ui, &mut asked);
        self.envelope_row(ui, &mut asked);
        let colour = if self.status_is_error {
            palette::error()
        } else {
            palette::text()
        };
        let status = self.status.clone();
        label_as(
            ui,
            "volumesStatus",
            &status,
            egui::Label::new(RichText::new(status.as_str()).color(colour.to_egui())).wrap(),
        );
        text_with(
            ui,
            "volumesReply",
            &mut self.reply,
            TextOptions {
                read_only: true,
                rows: 4,
                monospace: true,
            },
        );
        match asked {
            Some(Asked::Measure) => {
                self.measure(host);
            }
            Some(Asked::Copy) => self.copy(host),
            Some(Asked::Browse) => self.browse(host),
            Some(Asked::SaveCsv) => {
                self.save_csv(host);
            }
            Some(Asked::CutFillMap) => {
                self.make_cut_fill_map(host);
            }
            Some(Asked::Envelope) => {
                self.make_envelope(host);
            }
            None => {}
        }
    }

    fn footer(&mut self, ui: &mut Ui, _host: &mut dyn Host) {
        let close = ui
            .horizontal(|ui| button(ui, "volumesClose", "Close").clicked())
            .inner;
        if close {
            self.hide();
        }
    }
}
