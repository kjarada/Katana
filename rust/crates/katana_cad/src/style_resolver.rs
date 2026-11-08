//! One answer to "what does this NAME draw", for the viewport, the plot, and
//! every preview and thumbnail - so that none of them can disagree with the
//! others about what a style looks like (include/katana/cad/style_resolver.hpp,
//! src/katana_cad/customisation/style_resolver.cpp).
//!
//! A name reaches here from Style::linetype, Layer::linetype or Style::symbol.
//! Two tables can hold it: the Model's Linetype table (DXF-style dashes, saved
//! in the project) and the session's StyleLibrary (strokes, texts and pens;
//! Document::style_library). The rules, as the lead decided them:
//!
//! ```text
//! LINETYPE (decision D2). A NON-vertex library definition of the name wins
//! and is drawn by its own strokes, with NO Katana dash pattern applied to
//! them; otherwise a model Linetype gives dashes; otherwise the line is
//! solid. A name held by both tables is a COLLISION: reported, for a
//! diagnostics list, and never an error - a project can meet a library that
//! happens to reuse one of its names.
//!
//! SYMBOL. A library definition of the name, of either kind (most symbols
//! the reference mapfiles use are not `mode vertex`); otherwise one of the
//! sixteen built-in shapes, the name's own or the one its words suggest
//! (entity's built_in_symbol_for), and the caller is told which.
//!
//! SYMBOLS ON LINES (decision D8). A line whose style names a symbol draws
//! that symbol at EVERY vertex, as a vertex symbol is, and its name is never
//! used as a pattern along the line: a style whose linetype names its own
//! symbol - which is what an archive import writes for a symbol string - is a
//! plain line with symbols on it.
//! ```
//!
//! Nothing here keeps a reference into a library beyond the call that returned
//! it: the borrow checker holds what the C++ comment asked for (a LineStyle*
//! dangled as soon as Document::setStyleLibrary replaced the library).
//! Anything kept across frames is a FlatDefinition, owned, in a
//! DefinitionCache keyed on Document::library_generation().

use std::collections::BTreeMap;
use std::sync::Arc;

use katana_entity::{
    Geometry, LineStyle, Linetype, Model, Style, StyleLibrary,
    tables::{built_in_symbol_for, is_built_in_symbol_name, symbol_names},
};
use katana_geometry::{Box2, Point2, Polyline2};
use katana_math::cpp;
use katana_math::numerics::PI;

use crate::customisation_record::{
    RenamedDefinition, builtin_definition_renames, definition_name_now_in,
};
use crate::dashing::{DashOptions, for_each_dash};
use crate::style_drawing::{
    FlatDefinition, LinestyleLayoutOutcome, LinestyleOptions, StyleDrawing, StyleStroke,
    flatten_definition, style_drawing, symbol_drawing, symbol_drawing_of_style,
};
use crate::symbols::symbol_strokes;

// ---- linetypes ------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LinetypeKind {
    /// Drawn by `definition`'s strokes; no dash pattern at all.
    LibraryDefinition,
    /// Dashed by `linetype` (which may itself be continuous).
    ModelLinetype,
    /// A plain line: nothing holds the name.
    #[default]
    Solid,
}

/// What a linetype name resolves to. The references are into this call's
/// model and library only - see the module comment.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ResolvedLinetype<'a> {
    pub kind: LinetypeKind,
    /// LibraryDefinition.
    pub definition: Option<&'a LineStyle>,
    /// ModelLinetype.
    pub linetype: Option<&'a Linetype>,
    /// The model's Linetype table AND the library both hold the name. The
    /// library definition wins when it is not a vertex symbol; the model's
    /// linetype wins when it is. Either way a user reading "WATR Main" in two
    /// places deserves to be told.
    pub collision: bool,
}

/// The library definition `name` names: the exact name first, so a definition
/// that has that name itself - a person's own - is the one drawn; then, only
/// when that misses, the name builtin_definition_renames gives it now, so a
/// drawing saved before the built-in customisation was given general names
/// still draws the definitions that were renamed (customisation_record.rs).
/// None when neither is in the library. Every lookup below goes through here.
pub fn find_definition<'a>(library: &'a StyleLibrary, name: &str) -> Option<&'a LineStyle> {
    find_definition_in(library, name, builtin_definition_renames())
}

/// find_definition with the rename table `renamed`: the C++ three-argument
/// `findDefinition`.
pub fn find_definition_in<'a>(
    library: &'a StyleLibrary,
    name: &str,
    renamed: &[RenamedDefinition],
) -> Option<&'a LineStyle> {
    if let Some(definition) = library.find(name) {
        return Some(definition);
    }
    // Only a miss asks the rename table, and definition_name_now answers empty
    // for a name it does not know - which find("") misses.
    let now = definition_name_now_in(name, renamed);
    if now.is_empty() {
        None
    } else {
        library.find(now)
    }
}

pub fn resolve_linetype<'a>(
    model: &'a Model,
    library: &'a StyleLibrary,
    name: &str,
) -> ResolvedLinetype<'a> {
    let mut resolved = ResolvedLinetype::default();
    if name.is_empty() {
        return resolved; // no name: nothing to look up, a plain line
    }
    let definition = find_definition(library, name);
    let linetype = model.linetypes.find(name);
    resolved.collision = definition.is_some() && linetype.is_some();
    if let Some(definition) = definition
        && !definition.at_vertices
    {
        resolved.kind = LinetypeKind::LibraryDefinition;
        resolved.definition = Some(definition);
    } else if let Some(linetype) = linetype {
        resolved.kind = LinetypeKind::ModelLinetype;
        resolved.linetype = Some(linetype);
    }
    resolved
}

/// The pattern a LINE is drawn with, given its resolved linetype and symbol
/// names (D8): resolve_linetype, except that a linetype naming the line's own
/// symbol draws a plain line.
pub fn resolve_line_pattern<'a>(
    model: &'a Model,
    library: &'a StyleLibrary,
    linetype: &str,
    symbol: &str,
) -> ResolvedLinetype<'a> {
    if !symbol.is_empty() && linetype == symbol {
        // An archive import names a symbol string's style, linetype AND symbol
        // after the symbol. Taken as a linetype, the 147 symbols that are not
        // `mode vertex` were laid along their lines as patterns (D8).
        return ResolvedLinetype::default();
    }
    resolve_linetype(model, library, linetype)
}

// ---- symbols --------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SymbolKind {
    /// NO_SYMBOL: the viewport's plain point mark.
    #[default]
    None,
    /// `definition`.
    LibraryDefinition,
    /// The name IS one of the sixteen built-in shapes.
    BuiltIn,
    /// Nothing defines the name; `built_in` is the guess drawn.
    BuiltInFallback,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ResolvedSymbol<'a> {
    pub kind: SymbolKind,
    /// LibraryDefinition.
    pub definition: Option<&'a LineStyle>,
    /// The built-in shape drawn, one of entity's symbol_names(), for BuiltIn
    /// and BuiltInFallback; empty otherwise.
    pub built_in: &'static str,
}

// The viewport's plain point mark is four pixels either side of the point
// (kPointMarkerPixels in viewport_widget.cpp); a preview with no symbol size
// of its own draws a built-in shape the same size.
const PLAIN_MARK_PIXELS: f64 = 4.0;
// Enough chords that the sample's half circle reads as a curve at thumbnail
// size, and few enough that a pattern laid along it is not all corners.
const SAMPLE_ARC_CHORDS: i32 = 16;

/// The built-in shape `name` draws as: itself when it is one of the sixteen,
/// else the one its words suggest.
fn built_in_for(name: &str) -> ResolvedSymbol<'static> {
    if is_built_in_symbol_name(name) {
        // The name's own storage belongs to the caller; the built-in list's
        // does not move, so the answer points there.
        let shape = symbol_names()
            .iter()
            .copied()
            .find(|shape| *shape == name)
            .unwrap_or_default();
        ResolvedSymbol {
            kind: SymbolKind::BuiltIn,
            definition: None,
            built_in: shape,
        }
    } else {
        ResolvedSymbol {
            kind: SymbolKind::BuiltInFallback,
            definition: None,
            built_in: built_in_symbol_for(name),
        }
    }
}

/// A built-in shape's strokes as StyleStrokes in the entity's own pen, so one
/// painter draws both kinds of symbol.
fn built_in_drawing(
    shape: &str,
    at: Point2,
    size: f64,
    rotation: f64,
    fallback_half_width: f64,
) -> StyleDrawing {
    // The style's size is the symbol's WIDTH, as a library's is; the unit shape
    // takes a half-width.
    let half = if size > 0.0 {
        0.5 * size
    } else {
        fallback_half_width
    };
    let mut drawing = StyleDrawing::default();
    for stroke in symbol_strokes(shape, at, half, rotation) {
        drawing.strokes.push(StyleStroke {
            path: stroke,
            pen: String::new(),
        });
    }
    drawing
}

fn append(into: &mut StyleDrawing, from: StyleDrawing) {
    into.strokes.extend(from.strokes);
    into.texts.extend(from.texts);
}

/// The library first, so a library that defines "circle" is drawn as the
/// library says.
pub fn resolve_symbol<'a>(library: &'a StyleLibrary, name: &str) -> ResolvedSymbol<'a> {
    if name.is_empty() {
        return ResolvedSymbol::default();
    }
    if let Some(definition) = find_definition(library, name) {
        return ResolvedSymbol {
            kind: SymbolKind::LibraryDefinition,
            definition: Some(definition),
            built_in: "",
        };
    }
    built_in_for(name)
}

/// Flattened library definitions, kept from one frame to the next.
///
/// Keyed by (name, library generation): the first question asked at a new
/// generation empties the cache, so nothing flattened from a replaced library
/// is ever returned. What it hands out is a shared, owned FlatDefinition, never
/// a reference into the library, so an answer stays valid after the cache or
/// the library moves on. A name the library lacks is remembered too, as an
/// empty answer, since the built-in fallback asks for it on every point.
///
/// One cache per painter, on the thread that paints.
#[derive(Clone, Debug, Default)]
pub struct DefinitionCache {
    generation: Option<u64>,
    entries: BTreeMap<String, Option<Arc<FlatDefinition>>>,
}

impl DefinitionCache {
    pub fn find(
        &mut self,
        library: &StyleLibrary,
        generation: u64,
        name: &str,
    ) -> Option<Arc<FlatDefinition>> {
        if self.generation != Some(generation) {
            self.entries.clear();
            self.generation = Some(generation);
        }
        if let Some(found) = self.entries.get(name) {
            return found.clone();
        }
        let flat = find_definition(library, name)
            .map(|definition| Arc::new(flatten_definition(definition)));
        self.entries.insert(String::from(name), flat.clone());
        flat
    }

    /// The generation the entries belong to; absent before the first find.
    pub fn generation(&self) -> Option<u64> {
        self.generation
    }

    pub fn size(&self) -> usize {
        self.entries.len()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.generation = None;
    }
}

/// The strokes a point symbol draws, whichever table answers - library first,
/// then the built-in shape's strokes wrapped as StyleStrokes with the entity's
/// own pen - so a viewport and a preview cannot disagree about a symbol.
///
/// `size` is Style::symbol_size, a WIDTH in model units; 0 means the library
/// definition's own scale, or `fallback_half_width` for a built-in shape (the
/// caller's plain-mark size, in model units). Nothing is drawn for NO_SYMBOL.
pub fn point_symbol_drawing(
    library: &StyleLibrary,
    name: &str,
    at: Point2,
    size: f64,
    rotation: f64,
    paper_scale: f64,
    fallback_half_width: f64,
) -> StyleDrawing {
    let resolved = resolve_symbol(library, name);
    match resolved.kind {
        SymbolKind::None => return StyleDrawing::default(),
        SymbolKind::LibraryDefinition => {
            if let Some(definition) = resolved.definition {
                return symbol_drawing_of_style(definition, at, size, rotation, paper_scale);
            }
        }
        SymbolKind::BuiltIn | SymbolKind::BuiltInFallback => {}
    }
    built_in_drawing(resolved.built_in, at, size, rotation, fallback_half_width)
}

/// point_symbol_drawing with the library definition flattened once through
/// `cache`: the C++ overload that takes a DefinitionCache.
#[allow(
    clippy::too_many_arguments,
    reason = "The C++ pointSymbolDrawing's arguments, with the cache and the generation it is keyed on."
)]
pub fn point_symbol_drawing_cached(
    cache: &mut DefinitionCache,
    library: &StyleLibrary,
    generation: u64,
    name: &str,
    at: Point2,
    size: f64,
    rotation: f64,
    paper_scale: f64,
    fallback_half_width: f64,
) -> StyleDrawing {
    if name.is_empty() {
        return StyleDrawing::default();
    }
    if let Some(flat) = cache.find(library, generation, name) {
        return symbol_drawing(&flat, at, size, rotation, paper_scale);
    }
    built_in_drawing(
        built_in_for(name).built_in,
        at,
        size,
        rotation,
        fallback_half_width,
    )
}

// ---- level of detail and extent --------------------------------------------------

/// A symbol whose drawn extent is under this many pixels is drawn as a dot: at
/// that size its strokes are one smudge anyway, and a map of ten thousand coded
/// points zoomed out would otherwise lay out every stroke of every one.
pub const MINIMUM_SYMBOL_PIXELS: f64 = 3.0;

/// The box a drawing covers, each text counted by its height times its length
/// about its anchor - an over-estimate, since a text's real extent needs a font,
/// and the right way to be wrong for culling: a label reaching into the view
/// from an anchor outside it is drawn.
pub fn drawn_extent(drawing: &StyleDrawing) -> Box2 {
    let mut bounds = Box2::default();
    for stroke in &drawing.strokes {
        for &point in &stroke.path.vertices {
            bounds.expand(point);
        }
    }
    for text in &drawing.texts {
        let reach = text.height.abs() * text.text.len().max(1) as f64;
        bounds.expand_box(Box2::new(text.at, text.at).inflated(reach));
    }
    bounds
}

/// True when `drawing` is smaller than `minimum_pixels` across at `view_scale`
/// pixels per model unit, so the caller draws a dot instead. An empty drawing
/// is not "small": there is nothing to replace. MINIMUM_SYMBOL_PIXELS is the
/// C++ default of `minimum_pixels`.
pub fn below_symbol_detail(drawing: &StyleDrawing, view_scale: f64, minimum_pixels: f64) -> bool {
    if drawing.empty() || !(view_scale > 0.0) {
        return false;
    }
    let bounds = drawn_extent(drawing);
    cpp::max(bounds.width(), bounds.height()) * view_scale < minimum_pixels
}

/// Where a vertex symbol goes on an entity's plan shape (D8): every vertex of a
/// polyline, both ends of a segment or an arc, a point's own position. A circle
/// has no vertex and a text or a dimension is not a line, so they get none.
pub fn symbol_vertices(geometry: &Geometry) -> Vec<Point2> {
    match geometry {
        Geometry::Point(point) => vec![point.position],
        Geometry::Line(segment) => vec![segment.start, segment.end],
        Geometry::Arc(arc) => vec![arc.start_point(), arc.end_point()],
        Geometry::Polyline(polyline) => polyline.vertices.clone(),
        _ => Vec::new(),
    }
}

// ---- previews -------------------------------------------------------------------

/// The line a style is previewed along: a straight run, a sharp corner and a
/// half-circle arc, so a pattern is seen going straight, turning a corner and
/// bending round a curve. It fits the box from (0, 0) to (width, width / 4).
/// `vertices` are where a vertex symbol goes: the run's two ends, the corner
/// and the arc's end - not the chords the arc is drawn with, since a string's
/// vertices are the points it was surveyed at, not a curve's flattening.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StyleSamplePath {
    pub path: Polyline2,
    pub vertices: Vec<Point2>,
}

pub fn style_sample_path(width: f64) -> StyleSamplePath {
    // The run takes the first 45 % of the width, the corner climbs to the top
    // at 60 %, and the arc dips from there to the bottom of a circle of radius
    // 0.2 w about (0.8 w, 0.25 w) and back up to the right-hand edge.
    let w = cpp::max(width, 0.0);
    let mut sample = StyleSamplePath::default();
    let start = Point2::new(0.0, 0.0);
    let run_end = Point2::new(0.45 * w, 0.0);
    let corner = Point2::new(0.6 * w, 0.25 * w);
    let centre = Point2::new(0.8 * w, 0.25 * w);
    let radius = 0.2 * w;
    sample.path.vertices = vec![start, run_end, corner];
    for i in 1..=SAMPLE_ARC_CHORDS {
        // From 180 degrees round through 270 (the bottom) to 360.
        let angle = PI * (1.0 + f64::from(i) / f64::from(SAMPLE_ARC_CHORDS));
        sample.path.vertices.push(Point2::new(
            centre.x + radius * angle.cos(),
            centre.y + radius * angle.sin(),
        ));
    }
    let last = sample.path.vertices[sample.path.vertices.len() - 1];
    sample.vertices = vec![start, run_end, corner, last];
    sample
}

/// The plain line a sample draws when nothing patterns it.
fn solid(drawing: &mut StyleDrawing, path: &Polyline2) {
    if path.vertices.len() >= 2 {
        drawing.strokes.push(StyleStroke {
            path: path.clone(),
            pen: String::new(),
        });
    }
}

/// What `style` draws along `sample`: a library linestyle REPLACES the line, a
/// model linetype gives its dash spans (a dot as a one-point stroke), anything
/// else is the solid line; and a symbol sits at every vertex (D8). A style
/// whose linetype is "ByLayer" has no layer here, so its sample is a plain
/// line. `fallback_half_width` is the built-in symbol's half-width for a style
/// with no symbol size; 0 (the C++ default) derives it from
/// `dash_options.view_scale` as the viewport's four-pixel plain mark.
pub fn style_sample_drawing(
    model: &Model,
    library: &StyleLibrary,
    style: &Style,
    sample: &StyleSamplePath,
    paper_scale: f64,
    dash_options: &DashOptions,
    fallback_half_width: f64,
) -> StyleDrawing {
    let mut drawing = StyleDrawing::default();
    let path = &sample.path;
    let pattern = resolve_line_pattern(model, library, &style.linetype, &style.symbol);
    match pattern.kind {
        LinetypeKind::LibraryDefinition => {
            let options = LinestyleOptions {
                paper_scale,
                ..LinestyleOptions::default()
            };
            let laid = pattern
                .definition
                .map(|definition| style_drawing(&flatten_definition(definition), path, &options));
            match laid {
                Some(laid) if laid.outcome == LinestyleLayoutOutcome::Laid => {
                    append(&mut drawing, laid.drawing);
                }
                // The same plain line the viewport draws in its place.
                _ => solid(&mut drawing, path),
            }
        }
        LinetypeKind::ModelLinetype => {
            let dashed = pattern.linetype.is_some_and(|linetype| {
                for_each_dash(
                    &path.vertices,
                    path.closed,
                    linetype,
                    dash_options,
                    |from, to| {
                        let vertices = if from == to {
                            vec![from]
                        } else {
                            vec![from, to]
                        };
                        drawing.strokes.push(StyleStroke {
                            path: Polyline2::new(vertices, false),
                            pen: String::new(),
                        });
                    },
                )
            });
            if !dashed {
                // for_each_dash emitted nothing: continuous, too fine or too long.
                solid(&mut drawing, path);
            }
        }
        LinetypeKind::Solid => solid(&mut drawing, path),
    }
    if !style.symbol.is_empty() {
        let half = if fallback_half_width > 0.0 {
            fallback_half_width
        } else if dash_options.view_scale > 0.0 {
            PLAIN_MARK_PIXELS / dash_options.view_scale
        } else {
            0.0
        };
        for &vertex in &sample.vertices {
            append(
                &mut drawing,
                point_symbol_drawing(
                    library,
                    &style.symbol,
                    vertex,
                    style.symbol_size,
                    0.0,
                    paper_scale,
                    half,
                ),
            );
        }
    }
    drawing
}
