//! A picture of what a style, a library linestyle or a library symbol draws, at a plot scale, on
//! paper or on the screen - the preview pane of the style manager and the symbol library. The port
//! of customisation/style_preview.
//!
//! Everything is painted through customisation::style_painter from the one resolver (cad's
//! style_resolver), so the preview draws what the viewport and the plot draw; it computes nothing
//! of its own about a definition.
//!
//! WHAT IS SHOWN
//!
//! * a Style (`set_style`): cad's style_sample_drawing along style_sample_path - a straight run, a
//!   sharp corner and an arc - with the style's symbol at every vertex (decision D8). A style
//!   whose linetype is ByLayer has no layer here and is drawn as a plain line.
//! * one library definition as a LINESTYLE (`set_linestyle`): laid along the same sample, as the
//!   resolver lays it for a line whose linetype names it - a definition drawn at vertices or an
//!   unknown name is a plain line, said in the notice.
//! * one definition as a SYMBOL (`set_symbol`): cad's point_symbol_drawing at the model origin -
//!   the insertion point - which is marked with a crosshair, with a warning when the origin lies
//!   outside what the symbol draws (it will sit away from the point it is put on).
//! * an UNSAVED definition (`set_definition`), either way: the definition editor previews what is
//!   typed before any of it is in the library. It is drawn from a library of its own that holds
//!   that definition alone, so the Document's library is neither read for it nor changed by it.
//!
//! SCALE. Lines are shown at their PRINTED size: a plot millimetre is PIXELS_PER_PAPER_MILLIMETRE
//! pixels (a 96-dpi screen), and the model units one plot millimetre covers are N / 1000 at 1:N
//! (`paper_scale`) - Katana drawings are in METRES. So a paper linestyle looks the same at every
//! scale while a world linestyle or a model linetype shrinks as N grows, which is exactly what
//! choosing a scale is for. A symbol is fitted to the pane instead (at 96 dpi a 3 mm symbol would
//! be 11 pixels), centred on its insertion point; the scale still sizes a paper symbol on the
//! ground, and the scale bar, always in ground metres, says how big it is.
//!
//! GROUND. Paper is white, and a white or near-white pen prints black there (cad's paper_colour,
//! decision D7) - the entity pen and every library pen. The screen is the viewport's dark ground.
//! With no colour of its own (ByLayer) the entity pen is a new layer's white: white on screen,
//! black on paper.
//!
//! Setters only: the preview reports nothing. It is painted again whenever it is drawn, so it
//! follows the Document with no listener's work, and draws nothing but its ground once the
//! Document has gone - which it knows by a registration it keeps for that alone.
//!
//! What is the C++'s and not here. The pane was a widget painted by its own paint event at the
//! size it had; here `grab` paints it into an image of a size given, and `ui` grabs it at the size
//! the frame gives the pane. The crosshair and the scale bar, which the C++ drew as aliased lines
//! a pixel wide between whole pixels, are filled as the rows and columns of pixels those lines
//! cover, so that they are the same pixels on every raster backend.

use egui::Ui;
use katana_cad::dashing::DashOptions;
use katana_cad::document::{Document, ListenerHandle};
use katana_cad::plot::{PlotSettings, paper_colour};
use katana_cad::style_drawing::StyleDrawing;
use katana_cad::style_resolver::{
    DefinitionCache, LinetypeKind, SymbolKind, point_symbol_drawing_cached, resolve_linetype,
    resolve_symbol, style_sample_drawing, style_sample_path,
};
use katana_cad::view_transform::ViewTransform;
use katana_core::text::{format_general, lowered};
use katana_entity::display::is_by_layer;
use katana_entity::tables::CONTINUOUS_LINETYPE;
use katana_entity::{LineStyle, Model, Style, StyleLibrary};
use katana_geometry::Point2;

use super::name_picker::UNDEFINED_NAME_COLOUR;
use super::style_painter::{
    CanvasRasterBackend, RasterBackend, StylePaintTarget, paint_style_drawing, painted_extent,
};
use crate::canvas::{
    Brush, Canvas, Color, Font, FontMetrics, HAlign, Image, Path, Pen, PenCap, PenJoin, PenStyle,
    PointF, RectF, RenderHint, TextFlags, VAlign,
};
use crate::drive::controls::custom;
use crate::drive::registry::{ControlKind, ControlRecord};
use crate::theme::palette;

/// The pane's object name.
pub const STYLE_PREVIEW: &str = "stylePreview";

/// The entity pen on screen, as the viewport's plain line and the thumbnails' ink
/// (definition_thumbnails, INK_PIXELS).
const SCREEN_INK_PIXELS: f64 = 1.5;
/// The crosshair's arms, and the warning mark's size, in pixels.
const CROSSHAIR_ARM_PIXELS: i32 = 10;
const WARNING_PIXELS: f64 = 16.0;
/// The least the pane is drawn at (QWidget::setMinimumSize).
const MINIMUM_SIZE: egui::Vec2 = egui::Vec2::new(160.0, 90.0);

/// The paper the preview prints on: only the colour rule is read from it.
fn paper_settings() -> &'static PlotSettings {
    static SETTINGS: std::sync::OnceLock<PlotSettings> = std::sync::OnceLock::new();
    SETTINGS.get_or_init(PlotSettings::default)
}

fn no_model() -> &'static Model {
    static NONE: std::sync::OnceLock<Model> = std::sync::OnceLock::new();
    NONE.get_or_init(Model::new)
}

/// A plain line by name - the linetype names that never name a pattern.
fn plain_linetype_name(name: &str) -> bool {
    if name.is_empty() || is_by_layer(name) {
        return true;
    }
    let word = lowered(name);
    word == CONTINUOUS_LINETYPE.as_bytes() || word == b"0" || word == b"1"
}

/// The longest 1-2-5 length (..., 0.5, 1, 2, 5, 10, ...) of at most `limit`.
fn nice_length_at_most(limit: f64) -> f64 {
    if limit.is_nan() || limit <= 0.0 || !limit.is_finite() {
        return 0.0;
    }
    let decade = 10.0_f64.powf(limit.log10().floor());
    for step in [5.0, 2.0, 1.0] {
        if step * decade <= limit {
            return step * decade;
        }
    }
    decade
}

fn metres_text(metres: f64) -> String {
    if metres >= 1.0 {
        return format!("{} m", format_general(metres, 6));
    }
    format!("{} mm", format_general(metres * 1000.0, 6))
}

/// What the picture is on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PreviewGround {
    /// White, pens by the paper colour rule.
    #[default]
    Paper,
    /// The viewport's dark ground.
    Screen,
}

/// What an unsaved definition is shown as.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DefinitionAs {
    Symbol,
    Linestyle,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
enum Showing {
    #[default]
    Nothing,
    Style,
    Symbol,
    Linestyle,
}

/// See the module comment.
pub struct StylePreview {
    // First, so it goes first: also how a paint knows the Document is still there.
    registration: ListenerHandle,
    showing: Showing,
    style: Style,
    name: String,
    symbol_size: f64,
    denominator: i32,
    ground: PreviewGround,

    /// `set_definition`'s: a library holding the unsaved definition alone, and a count of the
    /// definitions shown so. The cache below keys on a library's generation, and this library is a
    /// new one at every keystroke.
    unsaved: Option<StyleLibrary>,
    unsaved_generation: u64,

    definitions: DefinitionCache,
    view: ViewTransform,
    insertion: Option<PointF>,
    origin_outside: bool,
    scale_bar_metres: f64,
    notice: String,
    texture: Option<egui::TextureHandle>,
}

impl StylePreview {
    /// The plot scales offered, as the N of 1:N.
    pub const PLOT_SCALES: [i32; 7] = [100, 200, 250, 500, 1000, 2000, 5000];
    /// A plot millimetre on screen: 96 dpi, the logical resolution a desktop screen is given, so
    /// "printed size" is the same on every machine.
    pub const PIXELS_PER_PAPER_MILLIMETRE: f64 = 96.0 / 25.4;
    /// Room kept round the drawing, and below it for the scale bar, in pixels.
    pub const MARGIN_PIXELS: i32 = 12;
    pub const SCALE_BAR_PIXELS: i32 = 24;

    /// The Document supplies the model (for a style's model linetype) and the library, lent to
    /// each paint. It may be dropped before the preview.
    pub fn new(document: &Document) -> Self {
        Self {
            // Nothing to do when the Document notifies: the pane is painted again each time it is
            // drawn. The registration is how a paint knows the Document has gone.
            registration: document.add_listener(|| {}),
            showing: Showing::Nothing,
            style: Style::default(),
            name: String::new(),
            symbol_size: 0.0,
            denominator: 500,
            ground: PreviewGround::Paper,
            unsaved: None,
            unsaved_generation: 0,
            definitions: DefinitionCache::default(),
            view: ViewTransform::default(),
            insertion: None,
            origin_outside: false,
            scale_bar_metres: 0.0,
            notice: String::new(),
            texture: None,
        }
    }

    /// Back to the Document's library, for the setters that show a name.
    fn forget_unsaved(&mut self) {
        if self.unsaved.take().is_some() {
            // What it holds was flattened from the unsaved definition, under a count that is no
            // generation of the Document's library - but may equal one.
            self.definitions.clear();
        }
    }

    // ---- what to show; each replaces the last --------------------------------------------------

    /// The style is copied: a form previews its working copy before anything is applied.
    pub fn set_style(&mut self, style: &Style) {
        self.forget_unsaved();
        self.showing = Showing::Style;
        self.style = style.clone();
    }

    /// `size` is Style::symbol_size - a width in model units, 0 for the definition's own.
    pub fn set_symbol(&mut self, name: &str, size: f64) {
        self.forget_unsaved();
        self.showing = Showing::Symbol;
        self.name = String::from(name);
        self.symbol_size = size;
    }

    pub fn set_linestyle(&mut self, name: &str) {
        self.forget_unsaved();
        self.showing = Showing::Linestyle;
        self.name = String::from(name);
    }

    /// A definition that is not in the Document's library, or not yet as it is given here: shown
    /// as `set_symbol` or `set_linestyle` would show it once it was, a symbol at its own size.
    /// Copied. One the library would refuse (entity's validate) shows nothing, as `clear` does.
    pub fn set_definition(&mut self, definition: &LineStyle, shown_as: DefinitionAs) {
        // A library of its own, holding this definition alone: everything below draws a NAME found
        // in a library, and the Document's either lacks this one or holds it as it was before the
        // edit.
        let mut library = StyleLibrary::new();
        if library.add(definition.clone()).is_err() {
            self.clear();
            return;
        }
        self.unsaved = Some(library);
        // The cache would answer a second edit with the first's strokes: same name, and no
        // generation of a library to tell them apart by.
        self.unsaved_generation += 1;
        self.definitions.clear();
        self.showing = match shown_as {
            DefinitionAs::Symbol => Showing::Symbol,
            DefinitionAs::Linestyle => Showing::Linestyle,
        };
        self.name.clone_from(&definition.name);
        self.symbol_size = 0.0;
    }

    pub fn clear(&mut self) {
        self.forget_unsaved();
        self.showing = Showing::Nothing;
    }

    /// 1:N. Any positive N is accepted; PLOT_SCALES are the ones a dialog offers. Anything else
    /// is ignored.
    pub fn set_scale_denominator(&mut self, denominator: i32) {
        if denominator > 0 {
            self.denominator = denominator;
        }
    }

    pub fn scale_denominator(&self) -> i32 {
        self.denominator
    }

    /// Model units (metres) per plot millimetre: N / 1000.
    pub fn paper_scale(&self) -> f64 {
        f64::from(self.denominator) / 1000.0
    }

    pub fn set_ground(&mut self, ground: PreviewGround) {
        self.ground = ground;
    }

    pub fn ground(&self) -> PreviewGround {
        self.ground
    }

    // ---- the last layout: where things were painted, for a dialog's status line and for tests.
    // Worked out by a paint (so after a grab or a frame), for the size painted at.

    /// Model to pane pixels, over the area above the scale bar.
    pub fn view(&self) -> &ViewTransform {
        &self.view
    }

    /// Symbol only: where the insertion point (the model origin) is painted.
    pub fn insertion_point(&self) -> Option<PointF> {
        self.insertion
    }

    /// Symbol only: the origin lies outside the painted extent, and the warning mark is shown.
    pub fn origin_outside_extent(&self) -> bool {
        self.origin_outside
    }

    /// The scale bar's length in ground metres; 0 when none was drawn.
    pub fn scale_bar_metres(&self) -> f64 {
        self.scale_bar_metres
    }

    /// What the pane says about what it drew in place of the name, if anything: "Not defined -
    /// drawn as ...". Empty when it drew the name.
    pub fn notice(&self) -> &str {
        &self.notice
    }

    /// The ground, as painted.
    pub fn ground_colour(&self) -> Color {
        match self.ground {
            PreviewGround::Paper => Color::WHITE,
            PreviewGround::Screen => palette::viewport(),
        }
    }

    /// The ink, as painted.
    pub fn ink_colour(&self, colour: Option<katana_entity::Color>) -> Color {
        // ByLayer: a new layer's white (the screen's default ink).
        let mut ink = colour.unwrap_or_default();
        if self.ground == PreviewGround::Paper {
            ink = paper_colour(ink, paper_settings());
        }
        Color::rgba(ink.r, ink.g, ink.b, ink.a)
    }

    /// The insertion point's crosshair.
    pub fn insertion_mark_colour() -> Color {
        palette::accent()
    }

    // ---- the layouts ---------------------------------------------------------------------------

    fn lay_out_line(&mut self, document: &Document, width: i32, height: i32) -> StyleDrawing {
        // Printed size: a plot millimetre is PIXELS_PER_PAPER_MILLIMETRE pixels and covers
        // paper_scale() metres.
        let pixels_per_metre = Self::PIXELS_PER_PAPER_MILLIMETRE / self.paper_scale();
        let usable_width = f64::from((width - 2 * Self::MARGIN_PIXELS).max(1));
        let usable_height = f64::from((height - 2 * Self::MARGIN_PIXELS).max(1));
        // style_sample_path(w) fills a box w wide and w / 4 high.
        let sample_pixels = katana_math::cpp::min(usable_width, 4.0 * usable_height);
        let sample_width = sample_pixels / pixels_per_metre;
        self.view.resize(f64::from(width), f64::from(height));
        self.view.scale = pixels_per_metre;
        self.view.center = Point2::new(sample_width / 2.0, sample_width / 8.0);
        let sample = style_sample_path(sample_width);
        let dashes = DashOptions {
            view_scale: pixels_per_metre,
            ..DashOptions::default()
        };
        let paper_scale = self.paper_scale();

        let library = match &self.unsaved {
            Some(unsaved) => unsaved,
            None => document.style_library(),
        };
        if self.showing == Showing::Style {
            let model = document.model();
            let mut notes: Vec<String> = Vec::new();
            let linetype = &self.style.linetype;
            let defined_linetype = plain_linetype_name(linetype)
                || *linetype == self.style.symbol
                || model.linetypes.contains(linetype)
                || library.contains(linetype);
            if !defined_linetype {
                notes.push(format!(
                    "Linetype \"{linetype}\" is not defined: drawn as a solid line."
                ));
            }
            if !self.style.symbol.is_empty() {
                let symbol = resolve_symbol(library, &self.style.symbol);
                if symbol.kind == SymbolKind::BuiltInFallback {
                    notes.push(format!(
                        "Symbol \"{}\" is not defined: drawn as the built-in \"{}\".",
                        self.style.symbol, symbol.built_in
                    ));
                }
            }
            self.notice = notes.join(" ");
            return style_sample_drawing(
                model,
                library,
                &self.style,
                &sample,
                paper_scale,
                &dashes,
                0.0,
            );
        }

        // One library definition as a linestyle: what a line whose linetype names it draws, with
        // no model linetype of the same name in the way - this is a picture of the LIBRARY's
        // definition (as the thumbnails are).
        let resolved = resolve_linetype(no_model(), library, &self.name);
        if resolved.kind != LinetypeKind::LibraryDefinition {
            self.notice = if library.contains(&self.name) {
                format!(
                    "\"{}\" is an `at vertices` symbol, not a linestyle: a line naming it is \
                     drawn plain.",
                    self.name
                )
            } else {
                format!("\"{}\" is not defined: drawn as a solid line.", self.name)
            };
        }
        let linestyle = Style {
            name: String::from("linestyle"),
            linetype: self.name.clone(),
            ..Style::default()
        };
        style_sample_drawing(
            no_model(),
            library,
            &linestyle,
            &sample,
            paper_scale,
            &dashes,
            0.0,
        )
    }

    fn lay_out_symbol(
        &mut self,
        document: &Document,
        width: i32,
        height: i32,
        font: &Font,
        metrics: &dyn FontMetrics,
    ) -> StyleDrawing {
        let paper_scale = self.paper_scale();
        let (library, generation) = match &self.unsaved {
            Some(unsaved) => (unsaved, self.unsaved_generation),
            None => (document.style_library(), document.library_generation()),
        };
        let resolved = resolve_symbol(library, &self.name);
        if resolved.kind == SymbolKind::BuiltInFallback {
            self.notice = format!(
                "\"{}\" is not defined: drawn as the built-in \"{}\".",
                self.name, resolved.built_in
            );
        }
        // A built-in shape with no size of its own is given 2 mm of paper - any size would do,
        // since the pane is fitted to the symbol.
        let drawing = point_symbol_drawing_cached(
            &mut self.definitions,
            library,
            generation,
            &self.name,
            Point2::new(0.0, 0.0),
            self.symbol_size,
            0.0,
            paper_scale,
            2.0 * paper_scale,
        );

        // Centred on the insertion point, and scaled so the symbol's reach from it fits either
        // way: the crosshair is always the pane's centre, and an off-centre symbol shows as
        // off-centre.
        self.view.resize(f64::from(width), f64::from(height));
        self.view.center = Point2::new(0.0, 0.0);
        // Printed size, if nothing is drawn.
        self.view.scale = Self::PIXELS_PER_PAPER_MILLIMETRE / paper_scale;
        let extent = painted_extent(&drawing, font, metrics);
        self.origin_outside = false;
        if !extent.empty() {
            let reach_x = katana_math::cpp::max(extent.min.x.abs(), extent.max.x.abs());
            let reach_y = katana_math::cpp::max(extent.min.y.abs(), extent.max.y.abs());
            let usable_width = f64::from((width - 2 * Self::MARGIN_PIXELS).max(1));
            let usable_height = f64::from((height - 2 * Self::MARGIN_PIXELS).max(1));
            let mut scale = f64::INFINITY;
            if reach_x > 0.0 {
                scale = katana_math::cpp::min(scale, usable_width / (2.0 * reach_x));
            }
            if reach_y > 0.0 {
                scale = katana_math::cpp::min(scale, usable_height / (2.0 * reach_y));
            }
            if scale.is_finite() {
                self.view.scale = katana_math::cpp::clamp(
                    scale,
                    ViewTransform::MINIMUM_SCALE,
                    ViewTransform::MAXIMUM_SCALE,
                );
            }
            self.origin_outside = extent.min.x > 0.0
                || extent.max.x < 0.0
                || extent.min.y > 0.0
                || extent.max.y < 0.0;
        }
        let origin = self.view.world_to_screen(Point2::new(0.0, 0.0));
        self.insertion = Some(PointF::new(origin.x, origin.y));
        drawing
    }

    // ---- the paint -----------------------------------------------------------------------------

    /// Paints the pane, `width` by `height` logical pixels, on `canvas`: the C++'s paint event.
    /// `document` is the Document the preview was made with; it is not read once it has gone.
    pub fn paint(&mut self, canvas: &mut dyn Canvas, document: &Document, width: i32, height: i32) {
        let whole = RectF::new(0.0, 0.0, f64::from(width), f64::from(height));
        canvas.fill_rect(whole, &Brush::solid(self.ground_colour()));
        self.insertion = None;
        self.origin_outside = false;
        self.scale_bar_metres = 0.0;
        self.notice.clear();
        // The Document may have gone before this pane (a dialog closing with the application):
        // nothing of it may be read.
        if self.showing == Showing::Nothing || !self.registration.active() {
            return;
        }
        canvas.set_render_hint(RenderHint::Antialiasing, true);
        canvas.set_render_hint(RenderHint::TextAntialiasing, true);

        let area_height = (height - Self::SCALE_BAR_PIXELS).max(1);
        let drawing = if self.showing == Showing::Symbol {
            let font = canvas.font();
            self.lay_out_symbol(document, width, area_height, &font, canvas.metrics())
        } else {
            self.lay_out_line(document, width, area_height)
        };

        let paper = self.ground == PreviewGround::Paper;
        let colour = if self.showing == Showing::Style {
            self.style.color
        } else {
            None
        };
        // On paper a line weight is a printed width: the style's own, or for a bare definition the
        // weight a new style starts with.
        let line_weight = if self.showing == Showing::Style {
            self.style.line_weight
        } else {
            Style::default().line_weight
        };
        let ink_pixels = if paper {
            katana_math::cpp::max(1.0, line_weight * Self::PIXELS_PER_PAPER_MILLIMETRE)
        } else {
            SCREEN_INK_PIXELS
        };
        let target = StylePaintTarget {
            view: self.view,
            // Flat, as the viewport and the plot paint a definition: a dash is its own length, not
            // a pen width longer.
            entity_pen: Pen::with(
                self.ink_colour(colour),
                ink_pixels,
                PenStyle::Solid,
                PenCap::Flat,
                PenJoin::Bevel,
            ),
            paper: paper.then(paper_settings),
            entity_pen_only: false,
            // A pen inside the definition is the colour the drawing resolves its name to: the
            // session's customisation, then the standard names.
            colours: Some(&document.customisation_state().colours),
        };
        paint_style_drawing(canvas, &drawing, &target);

        if self.showing == Showing::Symbol {
            self.paint_insertion_mark(canvas, width);
        }
        self.scale_bar_metres = self.paint_scale_bar(canvas, width, area_height);
        if !self.notice.is_empty() {
            canvas.set_pen(Pen::new(UNDEFINED_NAME_COLOUR));
            canvas.draw_text_in(
                RectF::new(
                    f64::from(Self::MARGIN_PIXELS / 2),
                    2.0,
                    f64::from(width - Self::MARGIN_PIXELS),
                    f64::from(area_height),
                ),
                TextFlags::aligned(HAlign::Left, VAlign::Top).wrapped(),
                &self.notice,
            );
        }
    }

    /// A row or a column of whole pixels in `colour`: what an aliased line a pixel wide between
    /// whole pixels covers, its end pixel too (see the module comment).
    fn pixel_run(canvas: &mut dyn Canvas, x: i32, y: i32, across: i32, down: i32, colour: Color) {
        canvas.fill_rect(
            RectF::new(
                f64::from(x),
                f64::from(y),
                f64::from(across),
                f64::from(down),
            ),
            &Brush::solid(colour),
        );
    }

    fn paint_insertion_mark(&self, canvas: &mut dyn Canvas, width: i32) {
        let Some(insertion) = self.insertion else {
            return;
        };
        canvas.save();
        // Crisp and on whole pixels: a mark, not a drawing, and the one thing in the pane a
        // person lines things up against.
        canvas.set_render_hint(RenderHint::Antialiasing, false);
        let mark = Self::insertion_mark_colour();
        let x = insertion.x.round() as i32;
        let y = insertion.y.round() as i32;
        let arm = CROSSHAIR_ARM_PIXELS;
        Self::pixel_run(canvas, x - arm, y, 2 * arm + 1, 1, mark);
        Self::pixel_run(canvas, x, y - arm, 1, 2 * arm + 1, mark);
        if self.origin_outside {
            // A warning triangle in the top-right corner: the symbol will sit away from the point
            // it is put on.
            let corner = PointF::new(
                f64::from(width) - f64::from(Self::MARGIN_PIXELS) / 2.0 - WARNING_PIXELS,
                2.0,
            );
            let mut triangle = Path::new();
            triangle.move_to(PointF::new(corner.x + WARNING_PIXELS / 2.0, corner.y));
            triangle.line_to(PointF::new(
                corner.x + WARNING_PIXELS,
                corner.y + WARNING_PIXELS,
            ));
            triangle.line_to(PointF::new(corner.x, corner.y + WARNING_PIXELS));
            triangle.close_subpath();
            canvas.set_render_hint(RenderHint::Antialiasing, true);
            canvas.fill_path(&triangle, &Brush::solid(UNDEFINED_NAME_COLOUR));
            canvas.set_pen(Pen::new(Color::BLACK));
            canvas.draw_text_in(
                RectF::new(corner.x, corner.y, WARNING_PIXELS, WARNING_PIXELS + 2.0),
                TextFlags::CENTER,
                "!",
            );
        }
        canvas.restore();
    }

    /// The bar's length in ground metres, 0 when none fits. The bar's strip is the
    /// SCALE_BAR_PIXELS under `area_height`.
    fn paint_scale_bar(&self, canvas: &mut dyn Canvas, width: i32, area_height: i32) -> f64 {
        // About a quarter of the pane, in a 1-2-5 length of ground metres.
        let pixels_per_metre = self.view.scale;
        let metres = nice_length_at_most(f64::from(width) / 4.0 / pixels_per_metre);
        if metres.is_nan() || metres <= 0.0 {
            return 0.0;
        }
        let length = metres * pixels_per_metre;
        canvas.save();
        canvas.set_render_hint(RenderHint::Antialiasing, false);
        let ink = self.ink_colour(None);
        canvas.set_pen(Pen::new(ink));
        let left = Self::MARGIN_PIXELS;
        let right = Self::MARGIN_PIXELS + length.round() as i32;
        let top = area_height;
        let y = top + Self::SCALE_BAR_PIXELS / 2;
        Self::pixel_run(canvas, left, y, right - left + 1, 1, ink);
        Self::pixel_run(canvas, left, y - 4, 1, 9, ink);
        Self::pixel_run(canvas, right, y - 4, 1, 9, ink);
        canvas.draw_text_in(
            RectF::new(
                f64::from(right + 6),
                f64::from(top),
                f64::from(width - right - 6),
                f64::from(Self::SCALE_BAR_PIXELS),
            ),
            TextFlags::aligned(HAlign::Left, VAlign::VCenter),
            &format!("{}   at 1:{}", metres_text(metres), self.denominator),
        );
        canvas.restore();
        metres
    }

    /// The pane painted `width` by `height` logical pixels into an image, `ratio` device pixels a
    /// logical one (QWidget::grab): what `ui` shows, and what a test reads back.
    pub fn grab(&mut self, document: &Document, width: u32, height: u32, ratio: f64) -> Image {
        let ratio = if ratio.is_finite() && ratio > 0.0 {
            ratio
        } else {
            1.0
        };
        let device = |logical: u32| (f64::from(logical) * ratio).round().max(1.0) as u32;
        let mut image = Image::new(device(width), device(height));
        image.set_device_pixel_ratio(ratio);
        CanvasRasterBackend.paint_image(&mut image, &mut |canvas: &mut dyn Canvas| {
            self.paint(canvas, document, width as i32, height as i32);
        });
        image
    }

    /// Draws the pane `size` points, never smaller than its least size, as the control `name`
    /// (STYLE_PREVIEW, or the name its dialog gave it). `document` is the Document the preview was
    /// made with.
    pub fn ui(
        &mut self,
        ui: &mut Ui,
        name: &str,
        document: &Document,
        size: egui::Vec2,
    ) -> egui::Response {
        let size = size.max(MINIMUM_SIZE);
        let size = egui::vec2(size.x.round(), size.y.round());
        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
        if ui.is_rect_visible(rect) {
            let ratio = f64::from(ui.ctx().pixels_per_point());
            let image = self.grab(document, size.x as u32, size.y as u32, ratio);
            let handle = ui.ctx().load_texture(
                name,
                image.to_color_image(),
                egui::TextureOptions::LINEAR,
            );
            ui.painter().image(
                handle.id(),
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
            self.texture = Some(handle);
        }
        let mut record = ControlRecord::new(name, ControlKind::Other);
        record.text.clone_from(&self.notice);
        custom(ui, &response, record);
        response
    }
}
