//! Terrain > Surface Styles (terrain/surface_style_manager.hpp; docs/surface_display.md, "In the
//! window"): the drawing's surface display styles in one window - every field of a style
//! (katana_cad's surface_style) editable, a live preview of a sample surface drawn in it through
//! the plan view's own painter, New, Copy, Rename, Delete and Set Default, and a surface's style
//! chosen.
//!
//! NOTHING IS CHANGED HERE. Every edit builds the SURFACESTYLE (or SURFACE STYLE) line a person
//! would type and hands it to the window's one executor (`Host::run`): echoed in the command log,
//! kept in the history, undone as one step - and an agent does the same through the verbs. The form
//! is a buffer, as the Alignment Manager's grids are: edited freely, previewed as it is, applied as
//! one line naming only what changed.
//!
//! NON-MODAL, kept by the window. It hears the drawing through a DocumentWatcher, so a SURFACESTYLE
//! typed on the command line, an undo or an opened project reloads it at its next frame, keeping
//! the chosen style - and the form's unapplied edits while the style they were made to is as it
//! was.
//!
//! ```text
//! surfaceStyleManagerDialog  the dialog (the action terrainSurfaceStyles' data)
//! surfaceStyleList           the styles, by name
//! surfaceStyleDefaultNote    which style is the default
//! surfaceStyleName           the name New, Copy and Rename take
//! surfaceStyleNew            SURFACESTYLE NEW "name"
//! surfaceStyleCopy           SURFACESTYLE NEW "name" FROM "chosen"
//! surfaceStyleRename         SURFACESTYLE RENAME "chosen" "name"
//! surfaceStyleDelete         SURFACESTYLE DELETE "chosen"
//! surfaceStyleSetDefault     SURFACESTYLE DEFAULT "chosen"
//! surfaceStyleTabs           Plan, Colours, Contours, 3D
//! Plan:     surfaceStyleDescription, surfaceStyleTriangles, surfaceStyleTriangleColour,
//!           surfaceStyleBoundary, surfaceStyleBoundaryColour, surfaceStylePoints,
//!           surfaceStylePointColour, surfaceStyleBreaklines, surfaceStyleBreaklineColour,
//!           surfaceStyleFaces (none, flat, elevation, slope, aspect), surfaceStyleOpacity
//! Colours:  surfaceStyleRamp, surfaceStyleBanded, surfaceStyleBands, surfaceStyleStops
//!           ("v:#RRGGBB;v:#RRGGBB ..."), surfaceStyleMin, surfaceStyleMax (empty: the
//!           surface's own), surfaceStyleFlatColour
//! Contours: surfaceStyleContours, surfaceStyleInterval, surfaceStyleMajor, surfaceStyleBase,
//!           surfaceStyleMinorColour, surfaceStyleMajorColour, surfaceStyleLabels
//! 3D:       surfaceStyleShading (automatic, shaded, edges, wireframe, hidden),
//!           surfaceStyleFaces3d (flat, elevation, slope, aspect)
//! surfaceStylePreview        a sample surface drawn in the form's style, with its legend
//! surfaceStyleLine           the SURFACESTYLE SET line Apply would run, or why none
//! surfaceStyleApply          runs it
//! surfaceStyleRevert         the form back to the drawing's style
//! surfaceStyleSurface        the drawing's surfaces
//! surfaceStyleAssign         SURFACE STYLE "surface" "chosen"
//! surfaceStyleStatus         what the last line replied, or why it was refused
//! surfaceStyleClose
//! ```
//!
//! THE DRAWING IS THE HOST'S. The C++ dialog kept a pointer to the Document; here the drawing is
//! lent with the host each frame, so what reads it - `apply_line`, `form_style`, `select_style` -
//! is handed it.
//!
//! THE PREVIEW IS AN IMAGE. The C++ preview was a widget painted on demand; here the sample is
//! painted on the raster canvas when the form's style changed - not every frame - and kept as a
//! texture. `preview_image` paints the same picture at any size, which is what a test reads where
//! the C++ test grabbed the widget.

use std::ops::RangeInclusive;
use std::sync::{Arc, OnceLock};

use egui::{RichText, Ui};
use katana_cad::Document;
use katana_cad::colour_lookup::resolve_colour;
use katana_cad::terrain::surface_plan::{
    SurfacePlanCache, SurfacePlanStats, SurfacePlanView, plan_image_grid, surface_plan,
};
use katana_cad::terrain::surface_style::{
    ColourStop, MAXIMUM_SURFACE_BANDS, SurfaceDisplayStyle, SurfaceFaces, SurfaceStyle,
    surface_faces_word, surface_ramp_names,
};
use katana_cad::view_transform::ViewTransform;
use katana_core::error::make_error;
use katana_core::text::{equals_ignoring_case, parse_finite_double};
use katana_core::{ErrorCode, Result};
use katana_entity::Color as EntityColor;
use katana_geometry::Point3;
use katana_terrain::{NamedSurface, TinSurface};

use crate::canvas::{
    Brush, Canvas, FontSize, Image, Pen, PointF, RasterCanvas, RectF, RenderHint, TextFlags,
};
use crate::command_word::{command_word, exact_number};
use crate::customisation::DocumentWatcher;
use crate::drive::controls::custom;
use crate::drive::mailbox::{Act, fill_disabled, lists_no, no_button};
use crate::drive::registry::{ControlKind, ControlRecord, record, settle, take};
use crate::drive::{
    Choice, LineOptions, SpinRange, button, check, choice, label, label_as, line, line_with, spin,
    tabs, whole,
};
use crate::geo::terrain_dialog_parts::{Framed, row, show_framed};
use crate::host::Host;
use crate::shell::DialogFrame;
use crate::survey::survey_dialogs::{message_colour, offstage};
use crate::terrain::surface_plan_painter::{
    SurfacePaintOptions, paint_surface_legend, paint_surface_plan,
};
use crate::theme::palette;

// ---- the words of the SURFACESTYLE verbs --------------------------------------------------------

const PLAN_FACES: [SurfaceFaces; 5] = [
    SurfaceFaces::None,
    SurfaceFaces::Flat,
    SurfaceFaces::Elevation,
    SurfaceFaces::Slope,
    SurfaceFaces::Aspect,
];
// faces3d= takes no "none": a surface in 3D always has faces (or is hidden by its shading).
const FACES_3D: [SurfaceFaces; 4] = [
    SurfaceFaces::Flat,
    SurfaceFaces::Elevation,
    SurfaceFaces::Slope,
    SurfaceFaces::Aspect,
];
const SHADINGS: [SurfaceStyle; 5] = [
    SurfaceStyle::Automatic,
    SurfaceStyle::Shaded,
    SurfaceStyle::ShadedWithEdges,
    SurfaceStyle::Wireframe,
    SurfaceStyle::Hidden,
];
/// The most the SURFACESTYLE verb takes for major= (surface_verbs' count_of): the box offers every
/// count the line does.
const MAXIMUM_MAJOR_EVERY: i64 = 1_000_000;
/// The opacity box: 0 to 1 in steps of 0.05, shown to two places.
const OPACITY: SpinRange = SpinRange::new(0.0, 1.0, 0.05, 2);

const TABS: [&str; 4] = ["Plan", "Colours", "Contours", "3D"];

// What each colour field is, as its hover text says it and a refusal names it.
const TRIANGLE_COLOUR_TIP: &str = "The triangles' colour";
const BOUNDARY_COLOUR_TIP: &str = "The boundary's colour";
const BREAKLINE_COLOUR_TIP: &str = "The breaklines' colour";
const POINT_COLOUR_TIP: &str = "The points' colour";
const FLAT_COLOUR_TIP: &str = "The colour of flat faces, and of a level face by aspect";
const MINOR_COLOUR_TIP: &str = "The minor contours' colour";
const MAJOR_COLOUR_TIP: &str = "The major contours' colour";

/// The preview's size, in points: the C++ widget's minimum.
const PREVIEW_WIDTH: u32 = 320;
const PREVIEW_HEIGHT: u32 = 220;
/// The most of the list shown before it scrolls.
const LIST_HEIGHT: f32 = 220.0;

/// The range of the bands box: the range the style check holds a style to
/// (MAXIMUM_SURFACE_BANDS), so the form, the line and a project file hold one range and no typed
/// count is clamped.
pub fn surface_style_bands_range() -> RangeInclusive<i64> {
    2..=i64::from(MAXIMUM_SURFACE_BANDS)
}

fn shading_word(shading: SurfaceStyle) -> &'static str {
    match shading {
        SurfaceStyle::Automatic => "automatic",
        SurfaceStyle::Shaded => "shaded",
        SurfaceStyle::ShadedWithEdges => "edges",
        SurfaceStyle::Wireframe => "wireframe",
        SurfaceStyle::Hidden => "hidden",
    }
}

fn colour_word(colour: EntityColor) -> String {
    colour.to_hex()
}

fn on_off(on: bool) -> &'static str {
    if on { "on" } else { "off" }
}

/// "v:#RRGGBB;v:#RRGGBB", the stops= value.
fn stops_word(stops: &[ColourStop]) -> String {
    stops
        .iter()
        .map(|stop| format!("{}:{}", exact_number(stop.value), colour_word(stop.colour)))
        .collect::<Vec<_>>()
        .join(";")
}

fn read_stops(text: &str) -> Result<Vec<ColourStop>> {
    let mut stops = Vec::new();
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(stops);
    }
    for part in trimmed.split(';').filter(|part| !part.is_empty()) {
        let read = part.split_once(':').and_then(|(value, colour)| {
            let value = parse_finite_double(value.trim())?;
            let colour = EntityColor::from_hex(colour.trim()).ok()?;
            Some(ColourStop { value, colour })
        });
        let Some(stop) = read else {
            return Err(make_error(
                ErrorCode::ParseFailure,
                "a stop is value:#RRGGBB, stops apart by ';'",
                part,
            ));
        };
        stops.push(stop);
    }
    Ok(stops)
}

/// A colour field: #RRGGBB, or a colour name the drawing knows - its customisation's, then the
/// standard names (cad's resolve_colour), as the verbs take it. The line always carries the colour
/// as #RRGGBB.
fn read_colour(field: &str, what: &str, document: Option<&Document>) -> Result<EntityColor> {
    let text = field.trim();
    if let Ok(colour) = EntityColor::from_hex(text) {
        return Ok(colour);
    }
    if let Some(document) = document
        && let Some(named) = resolve_colour(document, text)
    {
        return Ok(named);
    }
    Err(make_error(
        ErrorCode::ParseFailure,
        format!("{what} is #RRGGBB or a colour name"),
        field,
    ))
}

fn read_real(field: &str, what: &str) -> Result<f64> {
    parse_finite_double(field.trim())
        .ok_or_else(|| make_error(ErrorCode::ParseFailure, format!("{what} is a number"), field))
}

fn read_optional_real(field: &str, what: &str) -> Result<Option<f64>> {
    if field.trim().is_empty() {
        return Ok(None);
    }
    read_real(field, what).map(Some)
}

// ---- the words of an edit -----------------------------------------------------------------------

/// The SURFACESTYLE SET words that turn `from` into `to`, one key=value a field that differs, in
/// the order of the verb's keys - the line Apply runs, worked out apart from the dialog so it is
/// tested on its own. Empty when they are equal. Reals are written exactly (`exact_number`),
/// colours as #RRGGBB (#RRGGBBAA when not opaque), names through `command_word`. InvalidArgument
/// for what a line cannot say: a double quote or a line break in the description.
pub fn surface_style_set_words(
    from: &SurfaceDisplayStyle,
    to: &SurfaceDisplayStyle,
) -> Result<String> {
    let mut words: Vec<String> = Vec::new();
    let mut add = |key: &str, value: &str| words.push(format!("{key}={value}"));
    if from.description != to.description {
        // The verb reads a description as a typed text (core's unescape_typed): two backslashes
        // are one and a backslash before an n is a line break. Each backslash is doubled so the
        // text arrives as it was written in the box, and what SURFACESTYLE INFO prints reads back
        // the same.
        let typed = to.description.replace('\\', "\\\\");
        add("description", &command_word(&typed, "the description")?);
    }
    if from.show_triangles != to.show_triangles {
        add("triangles", on_off(to.show_triangles));
    }
    if from.triangle_colour != to.triangle_colour {
        add("trianglecolour", &colour_word(to.triangle_colour));
    }
    if from.show_boundary != to.show_boundary {
        add("boundary", on_off(to.show_boundary));
    }
    if from.boundary_colour != to.boundary_colour {
        add("boundarycolour", &colour_word(to.boundary_colour));
    }
    if from.show_points != to.show_points {
        add("points", on_off(to.show_points));
    }
    if from.point_colour != to.point_colour {
        add("pointcolour", &colour_word(to.point_colour));
    }
    if from.show_breaklines != to.show_breaklines {
        add("breaklines", on_off(to.show_breaklines));
    }
    if from.breakline_colour != to.breakline_colour {
        add("breaklinecolour", &colour_word(to.breakline_colour));
    }
    if from.plan_faces != to.plan_faces {
        add("faces", surface_faces_word(to.plan_faces));
    }
    if from.plan_opacity != to.plan_opacity {
        add("opacity", &exact_number(to.plan_opacity));
    }
    if from.contours.show != to.contours.show {
        add("contours", on_off(to.contours.show));
    }
    if from.contours.interval != to.contours.interval {
        add("interval", &exact_number(to.contours.interval));
    }
    if from.contours.major_every != to.contours.major_every {
        add("major", &format!("{}", to.contours.major_every));
    }
    if from.contours.base != to.contours.base {
        add("base", &exact_number(to.contours.base));
    }
    if from.contours.minor_colour != to.contours.minor_colour {
        add("minorcolour", &colour_word(to.contours.minor_colour));
    }
    if from.contours.major_colour != to.contours.major_colour {
        add("majorcolour", &colour_word(to.contours.major_colour));
    }
    if from.contours.label_major != to.contours.label_major {
        add("labels", on_off(to.contours.label_major));
    }
    if from.ramp != to.ramp {
        add("ramp", &command_word(&to.ramp, "the ramp")?);
    }
    if from.banded != to.banded {
        add("banded", on_off(to.banded));
    }
    if from.bands != to.bands {
        add("bands", &format!("{}", to.bands));
    }
    if from.stops != to.stops {
        add("stops", &stops_word(&to.stops));
    }
    if from.range_min != to.range_min {
        add("min", &to.range_min.map(exact_number).unwrap_or_default());
    }
    if from.range_max != to.range_max {
        add("max", &to.range_max.map(exact_number).unwrap_or_default());
    }
    if from.flat_colour != to.flat_colour {
        add("flatcolour", &colour_word(to.flat_colour));
    }
    if from.shading != to.shading {
        add("shading", shading_word(to.shading));
    }
    if from.faces3d != to.faces3d {
        add("faces3d", surface_faces_word(to.faces3d));
    }
    Ok(words.join(" "))
}

// ---- the sample the preview draws ---------------------------------------------------------------

/// A 120 x 80 m piece of ground, 24 x 16 cells: a hill, a hollow and a gentle fall across it, so
/// every kind of face shows - bands of height, steep and flat slopes, every aspect - with a
/// breakline across its middle row so a style's breaklines show too.
fn preview_surface() -> Arc<TinSurface> {
    static SAMPLE: OnceLock<Arc<TinSurface>> = OnceLock::new();
    Arc::clone(SAMPLE.get_or_init(|| {
        const COLUMNS: u32 = 24;
        const ROWS: u32 = 16;
        const CELL: f64 = 5.0;
        const BREAK_ROW: u32 = 8;
        let mut vertices = Vec::new();
        for j in 0..=ROWS {
            for i in 0..=COLUMNS {
                let x = f64::from(i) * CELL;
                let y = f64::from(j) * CELL;
                let hill =
                    6.0 * (-((x - 40.0) * (x - 40.0) + (y - 48.0) * (y - 48.0)) / 400.0).exp();
                let hollow =
                    4.0 * (-((x - 92.0) * (x - 92.0) + (y - 24.0) * (y - 24.0)) / 300.0).exp();
                vertices.push(Point3::new(x, y, 20.0 + hill - hollow + 0.04 * x));
            }
        }
        let at = |i: u32, j: u32| j * (COLUMNS + 1) + i;
        let mut triangles = Vec::new();
        let mut constrained: Vec<u8> = Vec::new();
        for j in 0..ROWS {
            for i in 0..COLUMNS {
                // (i,j) (i+1,j) (i+1,j+1), then (i,j) (i+1,j+1) (i,j+1): the first's edge 0 runs
                // along row j, the second's edge 1 along row j + 1 - the breakline's two sides.
                triangles.push([at(i, j), at(i + 1, j), at(i + 1, j + 1)]);
                constrained.push(if j == BREAK_ROW { 0b001 } else { 0 });
                triangles.push([at(i, j), at(i + 1, j + 1), at(i, j + 1)]);
                constrained.push(if j + 1 == BREAK_ROW { 0b010 } else { 0 });
            }
        }
        Arc::new(TinSurface::create(vertices, triangles, constrained).unwrap_or_default())
    }))
}

/// The form's style drawn on the sample, as the plan view would draw it, with its legend.
#[derive(Default)]
struct Preview {
    style: Option<SurfaceDisplayStyle>,
    cache: SurfacePlanCache,
    /// What the last paint drew of the sample.
    stats: SurfacePlanStats,
    /// The picture as the window last painted it, the ratio it was painted at, and whether the
    /// style has changed since.
    texture: Option<egui::TextureHandle>,
    ratio: f32,
    stale: bool,
}

impl Preview {
    fn show(&mut self, style: Option<SurfaceDisplayStyle>) {
        self.style = style;
        self.stale = true;
    }

    /// The preview painted on `canvas`, `width` by `height` of its logical units.
    fn paint(&mut self, canvas: &mut dyn Canvas, width: f64, height: f64) {
        let whole = RectF::new(0.0, 0.0, width, height);
        canvas.fill_rect(whole, &Brush::from(palette::viewport()));
        self.stats = SurfacePlanStats::default();
        let sample = preview_surface();
        let Some(style) = self.style.as_ref().filter(|_| !sample.empty()) else {
            canvas.set_pen(Pen::new(palette::text_muted()));
            canvas.draw_text_in(whole, TextFlags::CENTER, "Choose a style to see it here");
            return;
        };
        // The sample left of where the legend goes, when the style has one: a legend drawn over
        // the sample hid its corner.
        const LEGEND_ROOM: f64 = 130.0;
        let legend = style.plan_faces != SurfaceFaces::None;
        let mut view = ViewTransform::default();
        view.resize(
            (width - if legend { LEGEND_ROOM } else { 0.0 }).max(80.0),
            height,
        );
        view.fit(sample.bounds(), 0.06);
        let visible = view.visible_world_bounds();
        let ratio = canvas.device_pixel_ratio().max(1.0);
        let grid = plan_image_grid(
            &visible,
            sample.bounds(),
            1.0 / (view.scale * ratio),
            4_000_000,
        );
        let plan = SurfacePlanView {
            visible,
            units_per_pixel: 1.0 / view.scale,
            image_box: grid.r#box,
            image_width: grid.width,
            image_height: grid.height,
            // A label every 160 px: the sample is small, and the plan view's spacing would give
            // it one label at most.
            label_spacing_pixels: 160.0,
            ..SurfacePlanView::default()
        };
        let named = NamedSurface {
            name: String::from("sample"),
            surface: Some(sample),
            source: String::from("preview"),
            ..NamedSurface::default()
        };
        let drawn = surface_plan(&named, style, &plan, &mut self.cache);
        self.stats = drawn.stats;
        canvas.set_render_hint(RenderHint::Antialiasing, true);
        let options = SurfacePaintOptions {
            device_ratio: ratio,
            halo: palette::viewport(),
            ..SurfacePaintOptions::default()
        };
        paint_surface_plan(canvas, &drawn, &view, &options);
        if let Some(legend) = &drawn.legend {
            let mut font = canvas.font();
            font.size = FontSize::Pixels(10);
            let backing = palette::viewport().with_alpha(215);
            let _ = paint_surface_legend(
                canvas,
                legend,
                &style.name,
                PointF::new(width - 6.0, 6.0),
                &font,
                palette::text(),
                backing,
            );
        }
    }

    /// The preview as an image of `width` by `height` points at `ratio` device pixels a point.
    fn image(&mut self, width: u32, height: u32, ratio: f64) -> Image {
        let pixels = |points: u32| (f64::from(points) * ratio).round().max(1.0) as u32;
        let mut image = Image::new(pixels(width), pixels(height));
        image.set_device_pixel_ratio(ratio);
        let mut canvas = RasterCanvas::on(image);
        self.paint(&mut canvas, f64::from(width), f64::from(height));
        canvas.into_image()
    }
}

// ---- the dialog ---------------------------------------------------------------------------------

/// One style of the list, as the drawing had it at the last reload.
struct StyleRow {
    name: String,
    description: String,
    is_default: bool,
}

/// The form's fields: a buffer, edited freely.
#[derive(Default)]
struct Form {
    // Plan
    description: String,
    triangles: bool,
    triangle_colour: String,
    boundary: bool,
    boundary_colour: String,
    points: bool,
    point_colour: String,
    breaklines: bool,
    breakline_colour: String,
    faces: Choice,
    opacity: f64,
    // Colours
    ramp: Choice,
    banded: bool,
    bands: i64,
    stops: String,
    minimum: String,
    maximum: String,
    flat_colour: String,
    // Contours
    contours: bool,
    interval: String,
    major: i64,
    base: String,
    minor_colour: String,
    major_colour: String,
    labels: bool,
    // 3D
    shading: Choice,
    faces3d: Choice,
}

/// What a frame's clicks ask for, done after the dialog's body is drawn.
#[derive(Clone, Copy)]
enum Asked {
    New,
    Copy,
    Rename,
    Delete,
    SetDefault,
    Apply,
    Revert,
    Assign,
}

/// Terrain > Surface Styles (see the module comment).
pub struct SurfaceStyleManagerDialog {
    // First, so it goes first: no delivery reaches a half-dropped dialog.
    watcher: DocumentWatcher,
    frame: DialogFrame,
    visible: bool,

    styles: Vec<StyleRow>,
    /// The row of the list that is chosen.
    current: Option<usize>,
    default_note: String,
    name: String,
    tab: usize,
    faces_items: Vec<String>,
    ramp_items: Vec<String>,
    shading_items: Vec<String>,
    faces3d_items: Vec<String>,
    form: Form,
    preview: Preview,
    line: String,
    surfaces: Vec<String>,
    surface: Choice,
    status: String,
    status_is_error: bool,

    /// The drawing's style the form was filled from: what Apply changes, and what a reload
    /// compares with to keep unapplied edits.
    loaded: Option<SurfaceDisplayStyle>,
    /// What the opacity box showed when filled, rounded to its decimals.
    shown_opacity: f64,
    /// What the counts and the 3D faces showed when filled. A box clamps what it is given (major
    /// to its range) and the faces list has no "none"; the style keeps what it holds unless the
    /// person moves the box.
    shown_bands: i64,
    shown_major: i64,
    shown_faces3d: Option<usize>,
    /// The name to choose at the next reload (a style just made or renamed).
    pending: String,
}

fn strings(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| String::from(*word)).collect()
}

/// QComboBox::findText with Qt::MatchFixedString - the whole text, any case - else the first item.
fn item_of(items: &[String], text: &str) -> usize {
    items
        .iter()
        .position(|item| equals_ignoring_case(item.as_bytes(), text.as_bytes()))
        .unwrap_or(0)
}

impl SurfaceStyleManagerDialog {
    /// The dialog on the window's drawing, its default style chosen.
    pub fn new(host: &dyn Host) -> Self {
        let document = host.document();
        let mut dialog = Self {
            watcher: DocumentWatcher::new(document),
            frame: DialogFrame::new("surfaceStyleManagerDialog", "Surface Styles"),
            visible: false,
            styles: Vec::new(),
            current: None,
            default_note: String::new(),
            name: String::new(),
            tab: 0,
            faces_items: PLAN_FACES
                .iter()
                .map(|kind| String::from(surface_faces_word(*kind)))
                .collect(),
            ramp_items: strings(surface_ramp_names()),
            shading_items: SHADINGS
                .iter()
                .map(|kind| String::from(shading_word(*kind)))
                .collect(),
            faces3d_items: FACES_3D
                .iter()
                .map(|kind| String::from(surface_faces_word(*kind)))
                .collect(),
            form: Form {
                // A QComboBox given items shows its first.
                faces: Choice::at(0),
                ramp: Choice::at(0),
                shading: Choice::at(0),
                faces3d: Choice::at(0),
                // The boxes' lowest values, as a new box holds them.
                bands: *surface_style_bands_range().start(),
                ..Form::default()
            },
            preview: Preview::default(),
            line: String::new(),
            surfaces: Vec::new(),
            surface: Choice::default(),
            status: String::new(),
            status_is_error: false,
            loaded: None,
            shown_opacity: 0.0,
            shown_bands: 0,
            shown_major: 0,
            shown_faces3d: Some(0),
            pending: String::new(),
        };
        dialog.reload(document);
        dialog
    }

    /// The dialog's object name.
    pub fn object_name(&self) -> &str {
        self.frame.name()
    }

    pub fn show(&mut self) {
        self.visible = true;
    }

    pub fn hide(&mut self) {
        self.visible = false;
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    fn alive(&self) -> bool {
        self.watcher.document_alive()
    }

    fn set_status(&mut self, text: impl Into<String>, is_error: bool) {
        self.status = text.into();
        self.status_is_error = is_error;
    }

    /// What the last line replied, or why it was refused.
    pub fn status(&self) -> &str {
        &self.status
    }

    /// The drawing's style the list has chosen.
    fn chosen<'a>(&self, document: &'a Document) -> Option<&'a SurfaceDisplayStyle> {
        if !self.alive() {
            return None;
        }
        let row = self.current.and_then(|at| self.styles.get(at))?;
        document.surface_styles().find(&row.name)
    }

    /// Chooses style `name` in the list (any case); false when there is none.
    pub fn select_style(&mut self, name: &str, document: &Document) -> bool {
        let Some(at) = self
            .styles
            .iter()
            .position(|row| equals_ignoring_case(row.name.as_bytes(), name.as_bytes()))
        else {
            return false;
        };
        // A row already chosen is not chosen again: the form keeps its edits.
        if self.current != Some(at) {
            self.current = Some(at);
            self.choose_style(document);
        }
        true
    }

    /// The chosen style's name; empty when none is chosen.
    pub fn current_style(&self) -> String {
        self.current
            .and_then(|at| self.styles.get(at))
            .map(|row| row.name.clone())
            .unwrap_or_default()
    }

    /// The style the form says, unapplied edits and all: what the preview draws. None when no
    /// style is chosen, or a field does not read.
    pub fn form_style(&self, document: &Document) -> Option<SurfaceDisplayStyle> {
        self.read(document).ok()
    }

    /// What the last paint of the preview drew of the sample.
    pub fn preview_stats(&self) -> &SurfacePlanStats {
        &self.preview.stats
    }

    /// The preview as it is painted at `width` by `height` pixels: the sample in the form's style,
    /// with its legend (the C++ widget grabbed at that size).
    pub fn preview_image(&mut self, width: u32, height: u32) -> Image {
        self.preview.image(width, height, 1.0)
    }

    /// The list and the surfaces from the drawing, keeping what was chosen.
    fn reload(&mut self, document: &Document) {
        if !self.alive() {
            return;
        }
        let styles = document.surface_styles();
        let wanted = if !self.pending.is_empty() {
            std::mem::take(&mut self.pending)
        } else if let Some(row) = self.current.and_then(|at| self.styles.get(at)) {
            row.name.clone()
        } else {
            styles.default_style.clone()
        };
        self.pending.clear();
        self.styles = styles
            .styles
            .iter()
            .map(|style| StyleRow {
                name: style.name.clone(),
                description: style.description.clone(),
                is_default: equals_ignoring_case(
                    style.name.as_bytes(),
                    styles.default_style.as_bytes(),
                ),
            })
            .collect();
        self.current = if self.styles.is_empty() {
            None
        } else {
            Some(
                self.styles
                    .iter()
                    .position(|row| equals_ignoring_case(row.name.as_bytes(), wanted.as_bytes()))
                    .unwrap_or(0),
            )
        };
        self.default_note = format!(
            "Default: {} - a surface with no style of its own is drawn in it",
            styles.default_style
        );
        let was = self.surface.current_text(&self.surfaces);
        self.surfaces = document
            .surfaces()
            .all()
            .iter()
            .map(|named| named.name.clone())
            .collect();
        self.surface.index = self
            .surfaces
            .iter()
            .position(|name| *name == was)
            .or(if self.surfaces.is_empty() {
                None
            } else {
                Some(0)
            });
        // Keep unapplied edits while the style they were made to is as it was; else the form
        // shows the drawing's style.
        match self.chosen(document).cloned() {
            None => {
                self.loaded = None;
                self.preview.show(None);
            }
            Some(style) => {
                if self.loaded.as_ref() != Some(&style) {
                    self.fill(&style, document);
                }
            }
        }
    }

    /// The list's choice changed: the form shows the style chosen.
    fn choose_style(&mut self, document: &Document) {
        match self.chosen(document).cloned() {
            Some(style) => self.fill(&style, document),
            None => {
                self.loaded = None;
                self.preview.show(None);
            }
        }
    }

    fn fill(&mut self, style: &SurfaceDisplayStyle, document: &Document) {
        let form = &mut self.form;
        form.description.clone_from(&style.description);
        form.triangles = style.show_triangles;
        form.triangle_colour = colour_word(style.triangle_colour);
        form.boundary = style.show_boundary;
        form.boundary_colour = colour_word(style.boundary_colour);
        form.points = style.show_points;
        form.point_colour = colour_word(style.point_colour);
        form.breaklines = style.show_breaklines;
        form.breakline_colour = colour_word(style.breakline_colour);
        form.faces.index = Some(item_of(
            &self.faces_items,
            surface_faces_word(style.plan_faces),
        ));
        // A box shows a value rounded to its decimals and held to its range (setValue).
        form.opacity = OPACITY
            .rounded(style.plan_opacity)
            .clamp(OPACITY.minimum, OPACITY.maximum);
        self.shown_opacity = form.opacity;
        form.ramp.index = Some(item_of(&self.ramp_items, &style.ramp));
        form.banded = style.banded;
        let bands = surface_style_bands_range();
        form.bands = i64::from(style.bands).clamp(*bands.start(), *bands.end());
        self.shown_bands = form.bands;
        form.stops = stops_word(&style.stops);
        form.minimum = style.range_min.map(exact_number).unwrap_or_default();
        form.maximum = style.range_max.map(exact_number).unwrap_or_default();
        form.flat_colour = colour_word(style.flat_colour);
        form.contours = style.contours.show;
        form.interval = exact_number(style.contours.interval);
        form.major = i64::from(style.contours.major_every).clamp(0, MAXIMUM_MAJOR_EVERY);
        self.shown_major = form.major;
        form.base = exact_number(style.contours.base);
        form.minor_colour = colour_word(style.contours.minor_colour);
        form.major_colour = colour_word(style.contours.major_colour);
        form.labels = style.contours.label_major;
        form.shading.index = Some(item_of(&self.shading_items, shading_word(style.shading)));
        let faces3d = if style.faces3d == SurfaceFaces::None {
            SurfaceFaces::Flat
        } else {
            style.faces3d
        };
        form.faces3d.index = Some(item_of(&self.faces3d_items, surface_faces_word(faces3d)));
        self.shown_faces3d = form.faces3d.index;
        self.loaded = Some(style.clone());
        self.edited(document);
    }

    /// The form as a style, or why it is not one.
    fn read(&self, document: &Document) -> Result<SurfaceDisplayStyle> {
        let Some(loaded) = &self.loaded else {
            return Err(make_error(ErrorCode::InvalidState, "choose a style", ""));
        };
        let form = &self.form;
        let mut style = loaded.clone();
        style.description.clone_from(&form.description);
        style.show_triangles = form.triangles;
        style.show_boundary = form.boundary;
        style.show_points = form.points;
        style.show_breaklines = form.breaklines;
        style.plan_faces = PLAN_FACES
            .get(form.faces.index.unwrap_or(0))
            .copied()
            .unwrap_or(SurfaceFaces::None);
        // Exactly what the drawing holds unless the box was moved: it shows two decimals, and
        // read back as it shows it, 0.625 would be written over as 0.63. Moved is: not what
        // filling it showed.
        if form.opacity != self.shown_opacity {
            style.plan_opacity = form.opacity;
        }
        style.ramp = form.ramp.current_text(&self.ramp_items);
        style.banded = form.banded;
        if form.bands != self.shown_bands {
            style.bands = i32::try_from(form.bands).unwrap_or(style.bands);
        }
        style.contours.show = form.contours;
        if form.major != self.shown_major {
            style.contours.major_every =
                i32::try_from(form.major).unwrap_or(style.contours.major_every);
        }
        style.contours.label_major = form.labels;
        style.shading = SHADINGS
            .get(form.shading.index.unwrap_or(0))
            .copied()
            .unwrap_or(SurfaceStyle::Automatic);
        // A style read from a project can say faces3d=none, which the list shows as flat (the
        // verb has no such word): untouched, it stays.
        if form.faces3d.index != self.shown_faces3d {
            style.faces3d = FACES_3D
                .get(form.faces3d.index.unwrap_or(0))
                .copied()
                .unwrap_or(SurfaceFaces::Flat);
        }
        let names = self.alive().then_some(document);
        style.triangle_colour = read_colour(&form.triangle_colour, TRIANGLE_COLOUR_TIP, names)?;
        style.boundary_colour = read_colour(&form.boundary_colour, BOUNDARY_COLOUR_TIP, names)?;
        style.point_colour = read_colour(&form.point_colour, POINT_COLOUR_TIP, names)?;
        style.breakline_colour = read_colour(&form.breakline_colour, BREAKLINE_COLOUR_TIP, names)?;
        style.flat_colour = read_colour(&form.flat_colour, FLAT_COLOUR_TIP, names)?;
        style.contours.minor_colour = read_colour(&form.minor_colour, MINOR_COLOUR_TIP, names)?;
        style.contours.major_colour = read_colour(&form.major_colour, MAJOR_COLOUR_TIP, names)?;
        style.contours.interval = read_real(&form.interval, "the contour interval")?;
        style.contours.base = read_real(&form.base, "the base level")?;
        style.range_min = read_optional_real(&form.minimum, "the range's start")?;
        style.range_max = read_optional_real(&form.maximum, "the range's end")?;
        style.stops = read_stops(&form.stops)?;
        Ok(style)
    }

    /// A field changed: the preview and the line follow the form.
    fn edited(&mut self, document: &Document) {
        let shown = self.read(document).ok().or_else(|| self.loaded.clone());
        self.preview.show(shown);
        self.line = self.line_text(document);
    }

    /// The SURFACESTYLE SET line Apply would run; empty when the form says what the drawing holds,
    /// or cannot say it (surfaceStyleLine says why).
    pub fn apply_line(&self, document: &Document) -> String {
        let (Ok(style), Some(loaded)) = (self.read(document), &self.loaded) else {
            return String::new();
        };
        let (Ok(words), Ok(style_word)) = (
            surface_style_set_words(loaded, &style),
            command_word(&loaded.name, "the style's name"),
        ) else {
            return String::new();
        };
        if words.is_empty() {
            return String::new();
        }
        format!("SURFACESTYLE SET {style_word} {words}")
    }

    fn line_text(&self, document: &Document) -> String {
        let style = match self.read(document) {
            Ok(style) => style,
            Err(error) => return format!("Cannot apply: {}", error.describe()),
        };
        // A form that reads was filled from a style.
        let Some(loaded) = &self.loaded else {
            return String::new();
        };
        match surface_style_set_words(loaded, &style) {
            Err(error) => format!("Cannot apply: {}", error.describe()),
            Ok(words) if words.is_empty() => {
                String::from("The form says what the drawing holds: nothing to apply.")
            }
            Ok(_) => self.apply_line(document),
        }
    }

    /// Runs `text`, then reloads. The row a New, Copy or Rename names (`pending`) is chosen only
    /// if the line was accepted: a refused name (one that is taken) must leave the style being
    /// edited chosen, with its unapplied edits, not jump to the style that holds the name.
    fn run(&mut self, host: &mut dyn Host, text: &str) -> bool {
        let wanted = std::mem::take(&mut self.pending);
        if !self.alive() {
            self.set_status("The drawing this dialog edited is gone.", true);
            return false;
        }
        let outcome = host.run(text);
        if outcome.ok {
            let said = if outcome.reply.is_empty() {
                String::from(text)
            } else {
                outcome.reply
            };
            self.set_status(said, false);
            self.pending = wanted;
        } else {
            let said = if outcome.error.is_empty() {
                format!("Refused: {text}")
            } else {
                outcome.error
            };
            self.set_status(said, true);
        }
        self.reload(host.document());
        outcome.ok
    }

    /// A word for a name, or the status says why there is none.
    fn word(&mut self, text: &str, what: &str) -> Option<String> {
        match command_word(text, what) {
            Ok(word) => Some(word),
            Err(error) => {
                self.set_status(error.describe(), true);
                None
            }
        }
    }

    /// The chosen style's name as a word; None, with the status saying why, when it cannot be one
    /// - and, saying nothing, when no style is chosen.
    fn chosen_word(&mut self, document: &Document) -> Option<String> {
        let name = self.chosen(document)?.name.clone();
        self.word(&name, "the style's name")
    }

    fn make_style(&mut self, host: &mut dyn Host, from_chosen: bool) {
        let new_name = String::from(self.name.trim());
        let Some(new_word) = self.word(&new_name, "the new style's name") else {
            return;
        };
        let mut text = format!("SURFACESTYLE NEW {new_word}");
        if from_chosen {
            let Some(from_word) = self.chosen_word(host.document()) else {
                return;
            };
            text.push_str(&format!(" FROM {from_word}"));
        }
        self.pending = new_name;
        self.run(host, &text);
    }

    fn rename_style(&mut self, host: &mut dyn Host) {
        let new_name = String::from(self.name.trim());
        let from_word = self.chosen_word(host.document());
        let to_word = self.word(&new_name, "the new name");
        let (Some(from_word), Some(to_word)) = (from_word, to_word) else {
            return;
        };
        self.pending = new_name;
        self.run(host, &format!("SURFACESTYLE RENAME {from_word} {to_word}"));
    }

    fn delete_style(&mut self, host: &mut dyn Host) {
        if let Some(style_word) = self.chosen_word(host.document()) {
            self.run(host, &format!("SURFACESTYLE DELETE {style_word}"));
        }
    }

    fn default_style(&mut self, host: &mut dyn Host) {
        if let Some(style_word) = self.chosen_word(host.document()) {
            self.run(host, &format!("SURFACESTYLE DEFAULT {style_word}"));
        }
    }

    fn apply_form(&mut self, host: &mut dyn Host) {
        let text = self.apply_line(host.document());
        if text.is_empty() {
            let why = self.line_text(host.document());
            self.set_status(why, true);
            return;
        }
        self.run(host, &text);
    }

    fn assign_style(&mut self, host: &mut dyn Host) {
        let surface = self.surface.current_text(&self.surfaces);
        let surface_word = self.word(&surface, "the surface's name");
        let style_word = self.chosen_word(host.document());
        let (Some(surface_word), Some(style_word)) = (surface_word, style_word) else {
            return;
        };
        self.run(host, &format!("SURFACE STYLE {surface_word} {style_word}"));
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
            self.reload(host.document());
        }
        if show_framed(self, ui, host) {
            self.hide();
        }
    }

    // ---- what is drawn ----

    /// The styles, by name, the default in bold: a list named surfaceStyleList. A fill chooses
    /// the row of that name. True when the row chosen changed.
    fn style_list(&mut self, ui: &mut Ui) -> bool {
        const NAME: &str = "surfaceStyleList";
        let taken = take(ui, NAME);
        let mut changed = false;
        if let Some(taken) = &taken {
            match &taken.posted.act {
                Act::Fill(text) => {
                    if ui.is_visible() && !ui.is_enabled() {
                        settle(ui, taken, Err(fill_disabled(&taken.posted)));
                    } else if let Some(at) = self.styles.iter().position(|row| row.name == *text) {
                        changed = self.current != Some(at);
                        self.current = Some(at);
                        settle(ui, taken, Ok(()));
                    } else {
                        settle(ui, taken, Err(lists_no(NAME, text)));
                    }
                }
                Act::Press => settle(ui, taken, Err(no_button(&taken.posted))),
                Act::ScrollTo => {}
            }
        }
        let current = self.current;
        let mut picked = None;
        let styles = &self.styles;
        let rect = egui::ScrollArea::vertical()
            .id_salt(NAME)
            .max_height(LIST_HEIGHT)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for (at, row) in styles.iter().enumerate() {
                    let mut text = RichText::new(row.name.as_str());
                    if row.is_default {
                        text = text.strong();
                    }
                    let mut item = ui.selectable_label(current == Some(at), text);
                    if !row.description.is_empty() {
                        item = item.on_hover_text(row.description.as_str());
                    }
                    if item.clicked() {
                        picked = Some(at);
                    }
                }
            })
            .inner_rect;
        if let Some(at) = picked
            && self.current != Some(at)
        {
            self.current = Some(at);
            changed = true;
        }
        let response = ui
            .interact(rect, ui.id().with(NAME), egui::Sense::hover())
            .on_hover_text("The drawing's surface display styles");
        if let Some(taken) = &taken
            && taken.posted.act == Act::ScrollTo
        {
            response.scroll_to_me(None);
            settle(ui, taken, Ok(()));
        }
        let mut list = ControlRecord::new(NAME, ControlKind::Rows);
        list.items = self.styles.iter().map(|row| row.name.clone()).collect();
        list.value = self.current_style();
        list.enabled = ui.is_enabled();
        list.visible = ui.is_visible();
        record(ui, &response, list);
        changed
    }

    /// The styles and what is done to one by name.
    fn styles_column(&mut self, ui: &mut Ui, host: &mut dyn Host, asked: &mut Option<Asked>) {
        if self.style_list(ui) {
            self.choose_style(host.document());
        }
        let note = self.default_note.clone();
        label(ui, "surfaceStyleDefaultNote", &note);
        line_with(
            ui,
            "surfaceStyleName",
            &mut self.name,
            LineOptions {
                placeholder: "Name for New, Copy or Rename",
                ..LineOptions::default()
            },
        );
        let style_chosen = self.chosen(host.document()).is_some();
        let named = !self.name.trim().is_empty();
        let mut offer = |ui: &mut Ui, ready: bool, name: &str, text: &str, tip: &str, what| {
            let clicked = ui
                .add_enabled_ui(ready, |ui| {
                    button(ui, name, text).on_hover_text(tip).clicked()
                })
                .inner;
            if clicked {
                *asked = Some(what);
            }
        };
        ui.horizontal(|ui| {
            offer(
                ui,
                named,
                "surfaceStyleNew",
                "New",
                "A new style of that name, with the built-in defaults",
                Asked::New,
            );
            offer(
                ui,
                named && style_chosen,
                "surfaceStyleCopy",
                "Copy",
                "A new style of that name, a copy of the chosen one",
                Asked::Copy,
            );
            offer(
                ui,
                named && style_chosen,
                "surfaceStyleRename",
                "Rename",
                "The chosen style renamed; surfaces drawn in it follow",
                Asked::Rename,
            );
        });
        ui.horizontal(|ui| {
            offer(
                ui,
                style_chosen,
                "surfaceStyleDelete",
                "Delete",
                "Delete the chosen style (refused while a surface uses it)",
                Asked::Delete,
            );
            offer(
                ui,
                style_chosen,
                "surfaceStyleSetDefault",
                "Set Default",
                "Draw every surface with no style of its own in the chosen style",
                Asked::SetDefault,
            );
        });
    }

    /// A tick box and the colour of what it shows, in one row. True when either was edited.
    fn shown_row(
        ui: &mut Ui,
        names: (&str, &str),
        text: &str,
        tip: &str,
        on: &mut bool,
        colour: &mut String,
    ) -> bool {
        ui.horizontal(|ui| {
            let ticked = check(ui, names.0, text, on).changed();
            let typed = colour_field(ui, names.1, colour, tip);
            ticked || typed
        })
        .inner
    }

    fn plan_page(&mut self, ui: &mut Ui) -> bool {
        let form = &mut self.form;
        let mut edited = row(ui, "Description", |ui| {
            line(ui, "surfaceStyleDescription", &mut form.description).changed()
        });
        edited |= Self::shown_row(
            ui,
            ("surfaceStyleTriangles", "surfaceStyleTriangleColour"),
            "Triangles",
            TRIANGLE_COLOUR_TIP,
            &mut form.triangles,
            &mut form.triangle_colour,
        );
        edited |= Self::shown_row(
            ui,
            ("surfaceStyleBoundary", "surfaceStyleBoundaryColour"),
            "Boundary",
            BOUNDARY_COLOUR_TIP,
            &mut form.boundary,
            &mut form.boundary_colour,
        );
        edited |= Self::shown_row(
            ui,
            ("surfaceStyleBreaklines", "surfaceStyleBreaklineColour"),
            "Breaklines",
            BREAKLINE_COLOUR_TIP,
            &mut form.breaklines,
            &mut form.breakline_colour,
        );
        edited |= Self::shown_row(
            ui,
            ("surfaceStylePoints", "surfaceStylePointColour"),
            "Points",
            POINT_COLOUR_TIP,
            &mut form.points,
            &mut form.point_colour,
        );
        edited |= row(ui, "Faces", |ui| {
            choice(ui, "surfaceStyleFaces", &self.faces_items, &mut form.faces)
                .on_hover_text(
                    "What colours the faces in plan: nothing, one colour, the elevation, the \
                     slope or the aspect",
                )
                .changed()
        });
        edited |= row(ui, "Opacity", |ui| {
            let moved = spin(ui, "surfaceStyleOpacity", &mut form.opacity, OPACITY)
                .on_hover_text("How much the faces cover what is beneath them, 0 to 1")
                .changed();
            if moved {
                // The box holds what it shows: a value dragged to is rounded to its decimals.
                form.opacity = OPACITY.rounded(form.opacity);
            }
            moved
        });
        edited
    }

    fn colours_page(&mut self, ui: &mut Ui) -> bool {
        let form = &mut self.form;
        let mut edited = row(ui, "Ramp", |ui| {
            choice(ui, "surfaceStyleRamp", &self.ramp_items, &mut form.ramp).changed()
        });
        edited |= ui
            .horizontal(|ui| {
                let ticked = check(ui, "surfaceStyleBanded", "Banded", &mut form.banded).changed();
                let counted = whole(
                    ui,
                    "surfaceStyleBands",
                    &mut form.bands,
                    surface_style_bands_range(),
                )
                .on_hover_text("How many bands of equal width the range is cut into")
                .changed();
                ticked || counted
            })
            .inner;
        edited |= row(ui, "Stops", |ui| {
            line_with(
                ui,
                "surfaceStyleStops",
                &mut form.stops,
                LineOptions {
                    placeholder: "value:#RRGGBB; value:#RRGGBB ... (empty: the ramp)",
                    ..LineOptions::default()
                },
            )
            .on_hover_text(
                "Colours at values, in order; they replace the ramp. Banded, each is the lower \
                 edge of a band",
            )
            .changed()
        });
        let own = LineOptions {
            placeholder: "the surface's own",
            ..LineOptions::default()
        };
        edited |= row(ui, "Range from", |ui| {
            line_with(ui, "surfaceStyleMin", &mut form.minimum, own).changed()
        });
        edited |= row(ui, "Range to", |ui| {
            line_with(ui, "surfaceStyleMax", &mut form.maximum, own).changed()
        });
        edited |= row(ui, "Flat colour", |ui| {
            colour_field(
                ui,
                "surfaceStyleFlatColour",
                &mut form.flat_colour,
                FLAT_COLOUR_TIP,
            )
        });
        edited
    }

    fn contours_page(&mut self, ui: &mut Ui) -> bool {
        let form = &mut self.form;
        let mut edited = check(ui, "surfaceStyleContours", "Contours", &mut form.contours).changed();
        edited |= row(ui, "Interval", |ui| {
            line(ui, "surfaceStyleInterval", &mut form.interval).changed()
        });
        edited |= row(ui, "Major every", |ui| {
            whole(
                ui,
                "surfaceStyleMajor",
                &mut form.major,
                0..=MAXIMUM_MAJOR_EVERY,
            )
            .on_hover_text("Every Nth contour is a major; 0 for none")
            .changed()
        });
        edited |= row(ui, "Base level", |ui| {
            line(ui, "surfaceStyleBase", &mut form.base).changed()
        });
        edited |= row(ui, "Minor colour", |ui| {
            colour_field(
                ui,
                "surfaceStyleMinorColour",
                &mut form.minor_colour,
                MINOR_COLOUR_TIP,
            )
        });
        edited |= row(ui, "Major colour", |ui| {
            colour_field(
                ui,
                "surfaceStyleMajorColour",
                &mut form.major_colour,
                MAJOR_COLOUR_TIP,
            )
        });
        edited |= check(
            ui,
            "surfaceStyleLabels",
            "Label the majors",
            &mut form.labels,
        )
        .changed();
        edited
    }

    fn three_page(&mut self, ui: &mut Ui) -> bool {
        let form = &mut self.form;
        let mut edited = row(ui, "Shading", |ui| {
            choice(
                ui,
                "surfaceStyleShading",
                &self.shading_items,
                &mut form.shading,
            )
            .on_hover_text(
                "How the 3D view shades it: automatic draws edges up to 50 000 triangles",
            )
            .changed()
        });
        edited |= row(ui, "Faces", |ui| {
            choice(
                ui,
                "surfaceStyleFaces3d",
                &self.faces3d_items,
                &mut form.faces3d,
            )
            .changed()
        });
        edited
    }

    /// The chosen style's fields, the line they make, and Apply and Revert.
    fn form_column(&mut self, ui: &mut Ui, host: &mut dyn Host, asked: &mut Option<Asked>) {
        tabs(ui, "surfaceStyleTabs", &TABS, &mut self.tab);
        // The page in front takes the room; the others are still there, as a QTabWidget's are:
        // what they hold can be filled and read.
        let front = self.tab;
        let mut edited = false;
        for at in 0..TABS.len() {
            let draw = |ui: &mut Ui| match at {
                0 => self.plan_page(ui),
                1 => self.colours_page(ui),
                2 => self.contours_page(ui),
                _ => self.three_page(ui),
            };
            edited |= if at == front {
                ui.vertical(draw).inner
            } else {
                offstage(ui, &format!("surfaceStylePage{at}"), draw)
            };
        }
        if edited {
            self.edited(host.document());
        }
        let line = self.line.clone();
        label_as(
            ui,
            "surfaceStyleLine",
            &line,
            egui::Label::new(line.as_str()).wrap(),
        );
        let can_apply = !self.apply_line(host.document()).is_empty();
        let style_chosen = self.chosen(host.document()).is_some();
        ui.horizontal(|ui| {
            let apply = ui
                .add_enabled_ui(can_apply, |ui| {
                    button(ui, "surfaceStyleApply", "Apply")
                        .on_hover_text("Run the SURFACESTYLE SET line: one undo step")
                        .clicked()
                })
                .inner;
            let revert = ui
                .add_enabled_ui(style_chosen, |ui| {
                    button(ui, "surfaceStyleRevert", "Revert")
                        .on_hover_text("The form back to the drawing's style")
                        .clicked()
                })
                .inner;
            if apply {
                *asked = Some(Asked::Apply);
            }
            if revert {
                *asked = Some(Asked::Revert);
            }
        });
    }

    /// The preview: painted again when the form's style changed, or the screen's ratio did.
    fn preview_ui(&mut self, ui: &mut Ui) {
        let size = egui::vec2(PREVIEW_WIDTH as f32, PREVIEW_HEIGHT as f32);
        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
        if ui.is_rect_visible(rect) {
            let ratio = ui.ctx().pixels_per_point();
            if self.preview.stale
                || self.preview.texture.is_none()
                || self.preview.ratio.to_bits() != ratio.to_bits()
            {
                let image = self
                    .preview
                    .image(PREVIEW_WIDTH, PREVIEW_HEIGHT, f64::from(ratio));
                self.preview.texture = Some(ui.ctx().load_texture(
                    "surfaceStylePreview",
                    image.to_color_image(),
                    egui::TextureOptions::LINEAR,
                ));
                self.preview.ratio = ratio;
                self.preview.stale = false;
            }
            if let Some(texture) = &self.preview.texture {
                ui.painter().image(
                    texture.id(),
                    rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }
        }
        let response = response.on_hover_text(
            "A sample surface drawn in the style as the form says it, as the plan view draws it",
        );
        custom(
            ui,
            &response,
            ControlRecord::new("surfaceStylePreview", ControlKind::Other),
        );
    }

    /// A surface's style: the drawing's surfaces, and the chosen style given to one.
    fn assign_row(&mut self, ui: &mut Ui, host: &mut dyn Host, asked: &mut Option<Asked>) {
        let style_chosen = self.chosen(host.document()).is_some();
        ui.horizontal(|ui| {
            ui.label("Draw surface");
            choice(
                ui,
                "surfaceStyleSurface",
                &self.surfaces,
                &mut self.surface,
            )
            .on_hover_text("The drawing's surfaces");
            let ready = style_chosen && self.surface.index.is_some();
            let assign = ui
                .add_enabled_ui(ready, |ui| {
                    button(ui, "surfaceStyleAssign", "In This Style")
                        .on_hover_text("Run SURFACE STYLE: the surface drawn in the chosen style")
                        .clicked()
                })
                .inner;
            if assign {
                *asked = Some(Asked::Assign);
            }
        });
    }
}

/// A colour field: narrow, its placeholder the form a colour is typed in. True when it was edited.
fn colour_field(ui: &mut Ui, name: &str, value: &mut String, tip: &str) -> bool {
    line_with(
        ui,
        name,
        value,
        LineOptions {
            placeholder: "#RRGGBB",
            width: Some(110.0),
            ..LineOptions::default()
        },
    )
    .on_hover_text(tip)
    .changed()
}

impl Framed for SurfaceStyleManagerDialog {
    fn frame_mut(&mut self) -> &mut DialogFrame {
        &mut self.frame
    }

    fn body(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        let mut asked = None;
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_max_width(260.0);
                self.styles_column(ui, host, &mut asked);
            });
            ui.vertical(|ui| {
                ui.set_max_width(380.0);
                self.form_column(ui, host, &mut asked);
            });
            self.preview_ui(ui);
        });
        self.assign_row(ui, host, &mut asked);
        match asked {
            Some(Asked::New) => self.make_style(host, false),
            Some(Asked::Copy) => self.make_style(host, true),
            Some(Asked::Rename) => self.rename_style(host),
            Some(Asked::Delete) => self.delete_style(host),
            Some(Asked::SetDefault) => self.default_style(host),
            Some(Asked::Apply) => self.apply_form(host),
            Some(Asked::Revert) => {
                if let Some(style) = self.chosen(host.document()).cloned() {
                    self.fill(&style, host.document());
                }
            }
            Some(Asked::Assign) => self.assign_style(host),
            None => {}
        }
    }

    fn footer(&mut self, ui: &mut Ui, _host: &mut dyn Host) {
        let status = self.status.clone();
        let colour = message_colour(self.status_is_error);
        let close = ui
            .horizontal(|ui| {
                label_as(
                    ui,
                    "surfaceStyleStatus",
                    &status,
                    egui::Label::new(RichText::new(status.as_str()).color(colour)).wrap(),
                );
                button(ui, "surfaceStyleClose", "Close")
                    .on_hover_text("Close the window; unapplied edits are kept")
                    .clicked()
            })
            .inner;
        if close {
            self.hide();
        }
    }
}
