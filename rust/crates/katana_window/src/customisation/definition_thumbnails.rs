//! Small pictures of library definitions for the pickers and browsers: the symbol library, a
//! style's symbol and linestyle choosers, the survey-code manager (the port of
//! customisation/definition_thumbnails).
//!
//! Every picture is painted through the one style painter (style_painter) from the one resolver
//! (cad's style_resolver), so a thumbnail shows what the viewport would draw for the same name -
//! library definition first, the built-in shape a symbol name falls back to, the plain line a
//! linestyle name falls back to - and says when it is such a stand-in, so a picker can mark a name
//! the library does not define rather than drop it (decision D3).
//!
//! Pictures are cached by (kind, name, pixel size, ground colour), for one library generation.
//! The generation, never a definition's address, is what keeps a picture honest: the Document's
//! set_style_library moves it, and the first request after that drops every picture of the old
//! library. A picture bakes in the colours its pens resolved to, and the generation covers those
//! too: a Document's colour table moves it.
//!
//! On the UI thread, as the C++'s was: a picker asks for a few dozen pictures at a time - not
//! enough to be worth a farm. Pictures are painted through the window's raster backend
//! (style_painter's RasterBackend), as the C++ painted a QImage with a QPainter.

use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock};

use katana_cad::document::Document;
use katana_cad::plot::PlotSettings;
use katana_cad::style_drawing::{
    LinestyleLayoutOutcome, LinestyleOptions, StyleDrawing, StyleStroke, style_drawing,
};
use katana_cad::style_resolver::{
    DefinitionCache, LinetypeKind, SymbolKind, point_symbol_drawing_cached, resolve_linetype,
    resolve_symbol, style_sample_path,
};
use katana_entity::{ColourTable, Model, StyleLibrary};
use katana_geometry::Point2;

use crate::canvas::{Canvas, Color, Image, Pen, PenCap, PenJoin, PenStyle, RenderHint};
use crate::customisation::style_painter::{
    RasterBackend, StylePaintTarget, paint_style_drawing, painted_extent,
};

/// What a thumbnail shows a name as.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ThumbnailKind {
    /// The name drawn as a point symbol, centred.
    #[default]
    Symbol,
    /// The name drawn as a line's linetype along cad's style_sample_path.
    Linestyle,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DefinitionThumbnail {
    pub image: Image,
    /// What was painted is NOT a library definition of the name: a symbol name's built-in
    /// fallback shape, or the plain line a linestyle name the library does not hold (or holds only
    /// as a vertex symbol) is drawn as. False for a built-in shape asked for by its own name.
    pub stand_in: bool,
}

/// The entity pen's width in a picture: the viewport's own screen width, so a thumbnail's strokes
/// weigh what the drawing's do.
const INK_PIXELS: f64 = 1.5;
/// What a built-in shape with no size of its own is drawn at before the picture is fitted to it.
/// Any length does, since the fit scales it away.
const BUILT_IN_HALF_WIDTH: f64 = 1.0;
/// Repeats of a linestyle's period along the sample path, and how many times its reach across the
/// line the sample is wide: enough of the pattern to recognise, and room for a fence's ticks to
/// stand clear of the next ones.
const SAMPLE_PERIODS: f64 = 4.0;
const SAMPLE_REACHES: f64 = 8.0;
/// The share of each side left clear, so a pen at the edge is not cut in half.
const MARGIN_FRACTION: f64 = 0.12;

/// QColor::lightness(): the HSL lightness of Qt's 16-bit channels in single precision, back to
/// eight bits as Qt gives it.
fn lightness(colour: Color) -> i32 {
    let channel = |c: u8| f32::from(u16::from(c) * 0x101) / 65535.0;
    let (r, g, b) = (channel(colour.r), channel(colour.g), channel(colour.b));
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    // qRound of a non-negative float: half up.
    let sixteen = ((max + min) * 0.5 * 65535.0 + 0.5) as i32;
    // qt_div_257.
    (sixteen - (sixteen >> 8) + 0x80) >> 8
}

/// On a light ground the picture is on paper, and a light ground is one a light pen would vanish
/// into.
fn paper_ground(ground: Color) -> bool {
    lightness(ground) >= 128
}

fn no_model() -> &'static Model {
    static NONE: OnceLock<Model> = OnceLock::new();
    NONE.get_or_init(Model::new)
}

/// The paper the pictures print on: only the colour rule is read from it.
fn paper_settings() -> &'static PlotSettings {
    static SETTINGS: OnceLock<PlotSettings> = OnceLock::new();
    SETTINGS.get_or_init(PlotSettings::default)
}

/// The linestyle `name` laid along the sample path, or the plain sample path when the library does
/// not hold it as a line pattern. The same choice and the same laying as cad's style_sample_drawing
/// makes for a style naming it, without a Model: this is a picture of the LIBRARY's definition,
/// keyed on the library's generation, and a project's linetype of the same name is not.
fn linestyle_picture(
    definitions: &mut DefinitionCache,
    library: &StyleLibrary,
    generation: u64,
    name: &str,
) -> (StyleDrawing, bool) {
    // Only a NON-vertex definition is a line pattern (D2, D8); resolve_linetype with an empty
    // model answers exactly that, and never ModelLinetype for a name the empty model does not
    // hold.
    let resolved = resolve_linetype(no_model(), library, name);
    let flat = if resolved.kind == LinetypeKind::LibraryDefinition {
        definitions.find(library, generation, name)
    } else {
        None
    };
    let mut width = 1.0;
    if let Some(flat) = &flat {
        let period = if flat.length > 0.0 {
            flat.length
        } else {
            flat.natural_period()
        };
        width = katana_math::cpp::max3(
            SAMPLE_PERIODS * period,
            SAMPLE_REACHES * flat.reach_across,
            width,
        );
    }
    let sample = style_sample_path(width);
    if let Some(flat) = &flat {
        // Laid at any size (no view scale): a picture this small would call most patterns too
        // fine, and the point of it is to show the pattern.
        let laid = style_drawing(flat, &sample.path, &LinestyleOptions::default());
        if laid.outcome == LinestyleLayoutOutcome::Laid {
            return (laid.drawing, false);
        }
    }
    let plain = StyleDrawing {
        strokes: vec![StyleStroke {
            path: sample.path,
            pen: String::new(),
        }],
        ..StyleDrawing::default()
    };
    (plain, true)
}

/// The picture itself, uncached: what DefinitionThumbnails::thumbnail keeps. `size` is in DEVICE
/// pixels; an empty size gives a null image.
#[allow(
    clippy::too_many_arguments,
    reason = "The C++ paintDefinitionThumbnail's arguments, with the backend it paints through."
)]
pub fn paint_definition_thumbnail(
    raster: &dyn RasterBackend,
    definitions: &mut DefinitionCache,
    library: &StyleLibrary,
    generation: u64,
    kind: ThumbnailKind,
    name: &str,
    size: (u32, u32),
    ground: Color,
    colours: Option<&ColourTable>,
) -> DefinitionThumbnail {
    let mut result = DefinitionThumbnail::default();
    if size.0 == 0 || size.1 == 0 {
        return result;
    }
    let drawing = match kind {
        ThumbnailKind::Symbol => {
            result.stand_in = resolve_symbol(library, name).kind == SymbolKind::BuiltInFallback;
            point_symbol_drawing_cached(
                definitions,
                library,
                generation,
                name,
                Point2::new(0.0, 0.0),
                0.0,
                0.0,
                1.0,
                BUILT_IN_HALF_WIDTH,
            )
        }
        ThumbnailKind::Linestyle => {
            let (laid, stand_in) = linestyle_picture(definitions, library, generation, name);
            result.stand_in = stand_in;
            laid
        }
    };

    result.image = Image::new(size.0, size.1);
    result.image.fill(ground);
    if drawing.empty() {
        return result; // NO_SYMBOL, or an empty name: the ground alone
    }
    let paper = paper_ground(ground);
    let mut target = StylePaintTarget {
        // A definition is painted with a flat cap, as the viewport paints it, so a dash is its own
        // length and not a pen width longer.
        entity_pen: Pen::with(
            if paper {
                Color::BLACK
            } else {
                Color::rgb(224, 224, 224)
            },
            INK_PIXELS,
            PenStyle::Solid,
            PenCap::Flat,
            PenJoin::Bevel,
        ),
        paper: if paper { Some(paper_settings()) } else { None },
        colours,
        ..StylePaintTarget::default()
    };
    target.view.resize(f64::from(size.0), f64::from(size.1));
    raster.paint_image(&mut result.image, &mut |canvas: &mut dyn Canvas| {
        canvas.set_render_hint(RenderHint::Antialiasing, true);
        canvas.set_render_hint(RenderHint::TextAntialiasing, true);
        // Fitted to what is PAINTED, texts measured in their font: the culling extent
        // over-estimates a label threefold and left a labelled symbol a third of the picture.
        let extent = painted_extent(&drawing, &canvas.font(), canvas.metrics());
        let mut fitted = target.clone();
        fitted.view.fit(&extent, MARGIN_FRACTION);
        paint_style_drawing(canvas, &drawing, &fitted);
    });
    result
}

/// The key a picture is kept by: (kind, name, width, height, ground as RGBA). The generation is
/// not in the key: pictures of one generation only are ever held.
type Key = (ThumbnailKind, String, u32, u32, [u8; 4]);

/// The picture cache shared by every picker and browser the dialogs open.
pub struct DefinitionThumbnails {
    raster: Arc<dyn RasterBackend>,
    images: BTreeMap<Key, DefinitionThumbnail>,
    generation: Option<u64>,
    definitions: DefinitionCache,
}

impl DefinitionThumbnails {
    /// At most this many pictures are kept; past it the cache starts again. A picker of the
    /// reference libraries at two sizes is about 1,600.
    pub const MAXIMUM_ENTRIES: usize = 4096;

    /// A cache painting through `raster`, the window's raster backend.
    pub fn new(raster: Arc<dyn RasterBackend>) -> Self {
        Self {
            raster,
            images: BTreeMap::new(),
            generation: None,
            definitions: DefinitionCache::default(),
        }
    }

    /// A picture `size` device pixels across, on `ground`. The ink follows the ground: on a light
    /// ground the picture is "on paper" - the entity pen is black and a white library pen prints
    /// black (cad's paper_colour, decision D7) - and on a dark ground it is the screen, with a
    /// light entity pen. An empty size gives a null image.
    ///
    /// `colours` is the session's own colour names, which a pen inside a definition is resolved
    /// against before the standard ones (StylePaintTarget::colours); the Document form passes its
    /// customisation's. A caller that passes a table passes the generation that moves with it, as
    /// a Document's does.
    #[allow(
        clippy::too_many_arguments,
        reason = "The C++ thumbnail's arguments, as plain values."
    )]
    pub fn thumbnail(
        &mut self,
        library: &StyleLibrary,
        generation: u64,
        kind: ThumbnailKind,
        name: &str,
        size: (u32, u32),
        ground: Color,
        colours: Option<&ColourTable>,
    ) -> DefinitionThumbnail {
        if self.generation != Some(generation) {
            self.images.clear(); // every picture of the replaced library, at once
            self.generation = Some(generation);
        }
        let key: Key = (
            kind,
            String::from(name),
            size.0,
            size.1,
            [ground.r, ground.g, ground.b, ground.a],
        );
        if let Some(found) = self.images.get(&key) {
            return found.clone();
        }
        if self.images.len() >= Self::MAXIMUM_ENTRIES {
            self.images.clear();
        }
        let painted = paint_definition_thumbnail(
            self.raster.as_ref(),
            &mut self.definitions,
            library,
            generation,
            kind,
            name,
            size,
            ground,
            colours,
        );
        self.images.insert(key, painted.clone());
        painted
    }

    /// The same, for a Document's library, generation and colour names.
    pub fn thumbnail_of(
        &mut self,
        document: &Document,
        kind: ThumbnailKind,
        name: &str,
        size: (u32, u32),
        ground: Color,
    ) -> DefinitionThumbnail {
        self.thumbnail(
            document.style_library(),
            document.library_generation(),
            kind,
            name,
            size,
            ground,
            Some(&document.customisation_state().colours),
        )
    }

    pub fn size(&self) -> usize {
        self.images.len()
    }

    pub fn clear(&mut self) {
        self.images.clear();
        self.generation = None;
        self.definitions.clear();
    }
}
