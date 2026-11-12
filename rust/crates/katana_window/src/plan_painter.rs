//! The plan drawing, painted: one painter for the screen, the plot and every sheet viewport (the
//! port of plan_painter).
//!
//! What the plan view shows - imagery, point clouds, mesh footprints, every entity through its
//! layer and style, library linestyles and symbols, hatches, dimensions, text and the alignment
//! overlay - used to be painted by the viewport widget's own members, and a plot borrowed the live
//! widget to get at them: it swapped the view's transform for the sheet's, painted, and put it
//! back. That tied every plot to a widget on the GUI thread, gave it one view transform and no
//! rotation, and left the caches as widget members nobody else could share or own.
//!
//! paint_plan takes everything it needs as arguments - what to draw (PlanSource), where and how
//! it is looked at (PlanFrame: the transform, a rotation, the viewport on the device, the view's
//! hidden layers) and the medium (PlanPaintOptions: screen or paper, pixels per millimetre) - and
//! keeps what it learns between frames in a PlanPaintCache the CALLER owns. It reads no widget and
//! writes nothing but the canvas and the cache, so it is reentrant: two threads each with their
//! own cache may paint the same drawing onto an image and a PDF at once, which is what plotting a
//! sheet set in the background needs. (The model must not be edited while it is being painted; the
//! caller keeps it still, as the GUI thread does by painting between commands.)
//!
//! Screen and paper differ only where they must, and the difference is named here once: on screen
//! a line is a hairline, the selection and locked layers show, and marks are sized in pixels; on
//! paper a line is its layer's weight in millimetres, white prints black (cad's paper_colour), a
//! solid hatch is opaque, text is set at its exact fractional size, and every mark - point
//! crosses, the alignment overlay, hatch lines - is sized in paper millimetres, so a plot looks
//! the same at 150 dpi and at 600.
//!
//! On screen the painter also does what only a screen wants for speed, none of which changes what
//! a line looks like (docs/plan_view.md has the measurements): plain lines are clipped to the
//! view before the canvas sees them, each distinct layer/style/colour is resolved to its pens once
//! a frame instead of once an entity, and library symbols are stamped from images rasterised once
//! per symbol, pen and size. A plot stays vector.
//!
//! Imagery is the exception, and on paper it is embedded only as far as the viewport shows it and
//! only as finely as the plot can use: a raster is cropped to the viewport and averaged down to at
//! most PlanPaintOptions::raster_dpi_cap, and a point cloud is splatted into an image of the
//! viewport at that resolution, so a sheet over a 400-megapixel orthophoto embeds the few
//! megapixels it shows.
//!
//! The entry points the plan view (VP) and the sheet painters (SH1) call: paint_plan,
//! paint_plan_geometry (a tool's preview, the feedback painter's draw_shape), visible_box,
//! plan_drawn_bounds, plan_draws_in, sheet_frame, pdf_resolution_for and plot_plan_to_pdf, with
//! plan_source_of for a Document. What the Rust adds to the C++'s: PlanPaintOptions::raster (the
//! backend a sprite is rasterised by and text measured with; style_painter's RasterBackend) and
//! plot_plan_to_pdf's PdfPageWriter (the one-page PDF, C's PDF canvas in the window).

#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "The C++'s !(a > b) is kept: it rejects NaN, which a > b alone would let through."
)]

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use katana_cad::annotation::drawing::Drawing;
use katana_cad::annotation::label_layout::{
    LabelLayoutOptions, append_dimension_keep_out, append_keep_out, append_linework, layout_labels,
};
use katana_cad::annotation::leader_draw::build_leader;
use katana_cad::annotation::leader_presets::leader_context;
use katana_cad::annotation::text_layout::layout_text_entity;
use katana_cad::bim::bim_object::{BimModel, bim_model};
use katana_cad::bim::bim_view;
use katana_cad::dashing::{DashOptions, qt_dash_pattern};
use katana_cad::dimension_draw::{DimensionDrawing, build_dimension, resolve_dimension_style};
use katana_cad::document::Document;
use katana_cad::drawing::construction::is_construction_layer;
use katana_cad::hatching::{
    HatchDrawing, HatchOptions, hatch_drawing, hatch_segments, resolve_hatch_pattern_of_display,
};
use katana_cad::layer_overrides::{LayerOverrides, NO_LAYER_OVERRIDES};
use katana_cad::plot::{
    PlotColourMode, PlotSettings, millimetres_to_pixels, paper_colour, paper_dimensions,
    paper_fill_colour, sheet_for,
};
use katana_cad::plotting::arrange::draws_in;
use katana_cad::scene::SceneMesh;
use katana_cad::selection::{
    SelectionSet, drawn_extent as model_drawn_extent, is_ghost, touches_box,
};
use katana_cad::selection_style::{
    GHOST_ALPHA, GHOST_DOT_PITCH_PIXELS, GHOST_PEN_PIXELS, GHOST_RING_PIXELS, SELECTION_BLUE,
    SELECTION_GREEN, SELECTION_RED,
};
use katana_cad::spatial_query::for_each_candidate;
use katana_cad::style_drawing::{
    FlatDefinition, LinestyleLayoutOutcome, LinestyleOptions, style_drawing,
};
use katana_cad::style_resolver::{
    DefinitionCache, LinetypeKind, MINIMUM_SYMBOL_PIXELS, below_symbol_detail, drawn_extent,
    point_symbol_drawing_cached, resolve_line_pattern, symbol_vertices,
};
use katana_cad::terrain::surface_plan::{SurfacePlanCache, SurfaceProblem};
use katana_cad::terrain::surface_style::{SurfaceStyle, SurfaceStyleSet, built_in_surface_styles};
use katana_cad::view_transform::{DEFAULT_GRID_PIXELS, ViewTransform, grid_spacing};
use katana_core::error::make_error;
use katana_core::path_text::generic_path_utf8;
use katana_core::task_pool::TaskPool;
use katana_core::text::format_fixed;
use katana_core::{ErrorCode, Result, Status};
use katana_entity::annotation::{DEFAULT_ANNOTATION_SCALE, annotation_model_size};
use katana_entity::display::{ResolvedDisplay, resolve_display};
use katana_entity::entity_geometry::bounding_box;
use katana_entity::{
    Color as EntityColor, ColourTable, DimensionStyle, Entity, Geometry, HatchPattern,
    LeaderContext, Model, StyleLibrary, TextGeometry, TextJustify,
};
use katana_geometry::alignment::solve_alignment;
use katana_geometry::chording::{chord_arc, chord_circle};
use katana_geometry::point_splat::{
    SPLAT_POINT_BUDGET, SplatCloud, SplatView, build_splat_cloud, splat_cloud,
};
use katana_geometry::polygon::{clip, clip_polyline_with_mode};
use katana_geometry::{
    Arc2, Box2, Point2, Polyline2, PolylineClip, PolylineRuns, Segment2, SpatialIndex, Triangle2,
};
use katana_interop::reference_data::{
    PointCloudLayer, PointColorMode, RasterOverlay, ReferenceData, ReferenceId, color_for_point,
};
use katana_math::numerics::{PI, RAD_TO_DEG, TWO_PI};
use katana_math::unit_ratio::{to_metres, units};
use katana_render::{blue_of, green_of, red_of};
use katana_terrain::surface_store::SurfaceStore;

use crate::annotation::annotation_painter::{
    AnnotationFonts, AnnotationPaintTarget, paint_annotation_drawing,
};
use crate::canvas::{
    Brush, Canvas, Color, FillRule, Font, FontSize, Image, IntRect, Path as CanvasPath, Pen,
    PenCap, PenJoin, PenStyle, PointF, RectF, RenderHint, Transform,
};
use crate::customisation::style_painter::{RasterBackend, StylePaintTarget, paint_style_drawing};
use crate::drawing::feedback_painter::overlay;
use crate::plotting::plot_style::plot_image;
use crate::terrain::surface_plan_painter::{
    LegendStack, PaintedLegend, SurfacePaintOptions, paint_surface_legend_stack,
    paint_surfaces_in_plan,
};
use crate::theme::palette;

// ---- what is drawn, where, and how --------------------------------------------------------------

/// What is drawn. Every member but `model` may be absent, and each absence leaves out only what it
/// would have provided: no library draws symbols and linestyles as the built-in shapes and plain
/// lines, no index visits every entity, no selection draws nothing selected.
#[derive(Clone, Default)]
pub struct PlanSource<'a> {
    pub model: Option<&'a Model>,
    pub library: Option<&'a StyleLibrary>,
    /// The Document's library generation: what the cache's flattened definitions are keyed on,
    /// since a library can be replaced at the same address.
    pub library_generation: u64,
    /// The colour names of the session's customisation, which a pen inside a library definition
    /// is resolved against before the standard names (StylePaintTarget::colours). None: the
    /// standard names alone. A change of the table moves library_generation too, which is what
    /// drops a symbol stamp painted with the old colours.
    pub colours: Option<&'a ColourTable>,
    pub index: Option<&'a SpatialIndex>,
    pub selection: Option<&'a SelectionSet>,
    pub reference: Option<&'a ReferenceData>,
    pub meshes: Option<&'a [SceneMesh<'a>]>,
    /// What a leader's note reads beyond the entity its tip is on - the drawing's surfaces and
    /// alignments (cad's leader_context). None reads none: such a line of a note is dropped.
    pub leaders: Option<Arc<LeaderContext>>,
    /// The road model's objects, each drawn as its plan outline beneath the drawing (cad's
    /// bim_view has which are shown). None: none.
    pub road_model: Option<Arc<BimModel>>,
    /// Surface display (docs/surface_display.md): the drawing's surfaces and their display
    /// styles. Each visible surface is drawn in its style beneath the entities. None: no surfaces
    /// (the styles without the surfaces draw nothing; the surfaces without styles draw in the
    /// built-in ones).
    pub surfaces: Option<&'a SurfaceStore>,
    pub surface_styles: Option<&'a SurfaceStyleSet>,
}

/// Whether plan_source_of hands the road model to the plan views, plots and sheets: BIM > Show
/// Road Model, for the whole session, as View > Thin screen lines is. On until it is switched
/// off. Process-wide, as the C++'s atomic is: the window has one such switch.
static ROAD_MODEL_ON: AtomicBool = AtomicBool::new(true);

pub fn set_road_model_shown(shown: bool) {
    ROAD_MODEL_ON.store(shown, Ordering::Relaxed);
}

pub fn road_model_shown() -> bool {
    ROAD_MODEL_ON.load(Ordering::Relaxed)
}

/// The document's drawing, library, index, selection and what its leaders read, and its road
/// model while it is shown (set_road_model_shown). The reference data and meshes belong to the
/// window and are the caller's to add.
pub fn plan_source_of(document: &Document) -> PlanSource<'_> {
    PlanSource {
        model: Some(document.model()),
        library: Some(document.style_library()),
        library_generation: document.library_generation(),
        colours: Some(&document.customisation_state().colours),
        index: Some(document.spatial_index()),
        selection: Some(document.selection()),
        reference: None,
        meshes: None,
        leaders: Some(Arc::new(leader_context(document))),
        road_model: if road_model_shown() {
            Some(bim_model(document))
        } else {
            None
        },
        surfaces: Some(document.surfaces()),
        surface_styles: Some(document.surface_styles()),
    }
}

/// Where, and through what, the drawing is looked at.
#[derive(Clone, Copy, Default)]
pub struct PlanFrame<'a> {
    /// Model units to viewport pixels (y down), SIZED TO THE VIEWPORT: its centre is drawn at the
    /// viewport's centre. Unrotated.
    pub transform: ViewTransform,
    /// Counter-clockwise, in radians: the drawing turned on the device about the viewport's
    /// centre - a twisted viewport, as a sheet laid along an alignment is. 0 is north up.
    pub rotation: f64,
    /// The viewport's top-left corner on the device, in the canvas's units.
    pub origin: PointF,
    /// Nothing is painted outside the viewport's rectangle. Off for the screen, whose view clips
    /// already; on for a plot's printable area and for a viewport on a sheet, where the drawing
    /// must not run under the frame.
    pub clip: bool,
    /// The view's own hidden layers and reference layers; None hides nothing beyond what the
    /// document hides.
    pub layers: Option<&'a LayerOverrides>,
    pub hidden_references: Option<&'a BTreeSet<ReferenceId>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PlanMedium {
    #[default]
    Screen,
    Paper,
}

#[derive(Clone)]
pub struct PlanPaintOptions<'a> {
    pub medium: PlanMedium,
    /// Device pixels per millimetre. On paper, of the paper: what a line weight and every
    /// paper-sized mark is multiplied by. On screen, of the screen: what a paper linestyle's
    /// millimetres are, a screen line being a hairline whatever its weight.
    pub pixels_per_millimetre: f64,
    /// Screen only: the denominator N of 1:N for paper-sized library linestyles and symbols
    /// (mm * N / 1000 model metres). None keeps the automatic fixed screen size. Neither
    /// annotation nor plot scale.
    pub paper_style_scale: Option<f64>,
    /// On paper, the plot's settings, for the paper colour rule (D7). Required on paper; ignored
    /// on screen.
    pub plot: Option<&'a PlotSettings>,
    /// What is drawn besides the entities. The grid is screen furniture; the rest is drawn on both
    /// media, beneath the linework: rasters, then point clouds, then mesh footprints. On paper a
    /// footprint's outline is MESH_OUTLINE_PAPER_MILLIMETRES wide, dashed in paper millimetres.
    pub grid: bool,
    pub rasters: bool,
    pub point_clouds: bool,
    pub mesh_footprints: bool,
    pub alignments: bool,
    /// Paper only. The finest imagery is embedded at, in dots per inch of paper, and the most
    /// pixels one image may have. Imagery at the plot's own resolution would be enormous and no
    /// sharper to the eye: an A1 page at 300 dpi is 70 megapixels, 280 MB of RGBA before
    /// compression. So a raster is cropped to what the viewport shows (with a pixel of margin)
    /// and averaged down until one of its pixels is no finer than min(pixels_per_millimetre,
    /// raster_dpi_cap / 25.4) device pixels a millimetre - never enlarged - and then, if it is
    /// still above raster_pixel_cap, shrunk evenly to fit it. A point cloud is splatted into an
    /// image of the part of the viewport it covers at the same resolution - and no finer than
    /// CLOUD_POINT_PAPER_MILLIMETRES a pixel, a point's size - under the same pixel cap. 200 dpi
    /// is past what the eye resolves in a photograph at reading distance; 16 megapixels is A1 at
    /// 140 dpi and A0 at 100. Zero or less leaves the resolution uncapped.
    pub raster_dpi_cap: f64,
    pub raster_pixel_cap: usize,
    /// Screen only. Every line a cosmetic one-pixel pen instead of the 1.5 px hairline: measured
    /// 5-8x cheaper to stroke in Qt. Plots keep their paper-millimetre weights whatever this says.
    pub thin_lines: bool,
    /// Screen only. Library symbols stamped from images rasterised once per symbol, pen, size and
    /// quarter-pixel position, instead of stroked from scratch at every point. Plots stay vector.
    /// Ignored while the canvas's transform turns or scales (a rotated viewport), where a stamp
    /// would not land on the pixel grid it was rasterised for, and with no `raster` backend to
    /// rasterise a stamp with.
    pub symbol_sprites: bool,
    /// Screen only. Plain polylines, arcs and circles clipped to the view (with a margin wider
    /// than the pen reaches) before they are handed to the canvas, so a string crossing the whole
    /// site costs what its visible part costs. Off draws every vertex, which tests compare
    /// against.
    pub clip_lines: bool,
    /// At most this many cloud points projected a frame; above it every part of the view draws
    /// the same share of its points, coarsest first (geometry's point_splat), so a
    /// twenty-million-point cloud pans as a four-million-point one does. The same on paper, where
    /// the sheet editor repaints a cloud as the screen does.
    pub cloud_point_budget: usize,
    /// The face plain text (TextGeometry, dimension labels, the overlay) is drawn in. A library
    /// text names its own.
    pub font_family: String,
    /// The annotation scale, 1 : annotation_scale (docs/annotation.md): paper-sized text, labels,
    /// leaders and paper-sized dimensions are drawn at paper mm x annotation_scale / 1000 model
    /// units. The plan view passes the document's annotation scale, a plot its sheet's scale and
    /// a sheet viewport its own - never read from a widget.
    pub annotation_scale: f64,
    /// Labels, laid out for this scale by the placer (cad's label_layout), and whether it keeps
    /// them apart and off the lines.
    pub labels: bool,
    pub avoid_label_collisions: bool,
    /// What an annotation's mask is painted in on screen: the plan view's ground. On paper it is
    /// the paper.
    pub screen_background: Color,
    /// Screen only. A selected entity on a layer the document shows and the frame's layers hide
    /// is drawn as a GHOST - faint, dotted, in the selection colour, never picked or snapped to
    /// (cad's selection_style) - so a selection made in another view shows where it is here. Off
    /// by default: plots and every caller but the plan view draw what they always drew; the plan
    /// view sets it from its own switch.
    pub selection_ghosts: bool,
    /// Surface display (docs/surface_display.md). The drawing's surfaces, in their styles,
    /// beneath the entities; and on screen, in the view's top-right corner, the legend of each
    /// drawn surface whose style colours its faces. A plot carries no legend: a sheet's legend
    /// belongs with its title block, which is not done.
    pub surfaces: bool,
    pub surface_legends: bool,
    /// What a symbol sprite is rasterised with and annotation text measured with: the window's
    /// raster backend. None strokes every symbol (as a plot does) and measures text by cad's
    /// estimate. The C++ reaches both through Qt wherever it is.
    pub raster: Option<Arc<dyn RasterBackend>>,
}

impl Default for PlanPaintOptions<'_> {
    fn default() -> Self {
        Self {
            medium: PlanMedium::Screen,
            pixels_per_millimetre: 96.0 / 25.4,
            paper_style_scale: None,
            plot: None,
            grid: false,
            rasters: true,
            point_clouds: true,
            mesh_footprints: true,
            alignments: true,
            raster_dpi_cap: 200.0,
            raster_pixel_cap: 16_000_000,
            thin_lines: false,
            symbol_sprites: false,
            clip_lines: true,
            cloud_point_budget: SPLAT_POINT_BUDGET,
            font_family: String::from("Segoe UI"),
            annotation_scale: DEFAULT_ANNOTATION_SCALE,
            labels: true,
            avoid_label_collisions: true,
            screen_background: Color::rgb(0x1e, 0x23, 0x29),
            selection_ghosts: false,
            surfaces: true,
            surface_legends: true,
            raster: None,
        }
    }
}

/// What one paint did, for a view's statistics and for tests that cannot look at pixels.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlanPaintStats {
    /// Passed the visibility rule and lay in view.
    pub entities_drawn: usize,
    /// Selected entities drawn as ghosts (PlanPaintOptions::selection_ghosts): kept apart from
    /// entities_drawn, which proves a hidden layer is gone.
    pub ghosts_drawn: usize,
    pub symbols_stamped: usize,
    /// Of those, from a cached image.
    pub sprites_drawn: usize,
    /// Distinct layer/style/colour combinations resolved to pens this paint: once each, however
    /// many entities share one.
    pub displays_resolved: usize,
    /// Polylines, arcs and circles that ran off the view and were clipped.
    pub lines_clipped: usize,
    /// Point-cloud points in the parts of the clouds on the view, and of those how many were
    /// drawn: fewer only when the point budget thinned them.
    pub cloud_points_in_view: usize,
    pub cloud_points_drawn: usize,
    /// Label pieces the placer placed, of those how many away from their first place, and how many
    /// found no room (docs/annotation.md).
    pub labels_placed: usize,
    pub labels_displaced: usize,
    pub labels_suppressed: usize,
    /// Rasters drawn - visible, not hidden in this view and, on paper, with a part in the
    /// viewport - and on paper the pixels of the images put on the page for them: the crops as
    /// resampled, which is what a PDF embeds. Of those crops, how many were resampled this paint
    /// rather than taken from the cache.
    pub rasters_drawn: usize,
    pub raster_pixels_embedded: usize,
    pub raster_crops_made: usize,
    /// On paper, the pixels of the images the point clouds were splatted into.
    pub cloud_pixels_embedded: usize,
    /// Surface display (docs/surface_display.md): the surfaces drawn (visible, with something in
    /// view), their triangles in view, the edges stroked and the triangles too dense to stroke
    /// (tinted), the contour runs and labels, the pixels of their faces' images and how many they
    /// cover (cad's count of the faces, the record's covered=), and the legends drawn.
    pub surfaces_drawn: usize,
    pub surface_triangles_in_view: usize,
    pub surface_edges_stroked: usize,
    pub surface_triangles_tinted: usize,
    pub surface_contour_runs: usize,
    pub surface_labels: usize,
    pub surface_image_pixels: usize,
    pub surface_pixels_covered: usize,
    pub surface_legends: usize,
    /// Why part of a surface's style was not drawn, one for each surface that has a problem
    /// (contours too fine to trace are refused, naming the interval): empty when all of every
    /// style was. The plan view tells the person once, the headless report prints it, and a plot
    /// or a sheet viewport - which paints the same stats - passes it on to the plot's reply and
    /// the sheet's problems.
    pub surface_problems: Vec<SurfaceProblem>,
}

// Sizes of the marks the painter draws that are not the drawing's own: on screen in pixels, on
// paper in millimetres of paper, so that a plot's marks do not shrink as its resolution rises (a
// 4 px cross was 0.68 mm at 300 dpi and 0.34 mm at 600).
/// The plain point cross's half-width.
pub const POINT_MARKER_PIXELS: f64 = 4.0;
/// The same on paper: a 2 mm cross.
pub const POINT_MARKER_PAPER_MILLIMETRES: f64 = 1.0;
/// The finest ISO 128 pen.
pub const HATCH_LINE_PAPER_MILLIMETRES: f64 = 0.13;
// The alignment overlay: on screen a 2 px centreline, 1 px ticks 6 px either side, 11 px labels
// at least 70 px apart; on paper the nearest standard sizes, the label gap kept in proportion to
// the label.
pub const ALIGNMENT_LINE_PAPER_MILLIMETRES: f64 = 0.5;
pub const ALIGNMENT_TICK_PEN_PAPER_MILLIMETRES: f64 = 0.25;
pub const ALIGNMENT_TICK_PAPER_MILLIMETRES: f64 = 1.5;
pub const ALIGNMENT_TEXT_PAPER_MILLIMETRES: f64 = 2.5;
// A mesh footprint's outline: on screen a one-pixel dashed pen, on paper the finest ISO pen dashed
// 2 mm on, 1 mm off - the screen's 4 : 2 dash in millimetres, since a dash counted in pen widths
// of 0.13 mm would be a dot.
pub const MESH_OUTLINE_PAPER_MILLIMETRES: f64 = 0.13;
pub const MESH_DASH_PAPER_MILLIMETRES: f64 = 2.0;
pub const MESH_GAP_PAPER_MILLIMETRES: f64 = 1.0;
/// A point-cloud point of size 1 on paper: the size of a screen's pixel. The splat draws a point
/// as whole pixels of its image, so on paper that image is no finer than this a pixel. Finer, a
/// point was 0.13 mm at the 200 dpi cap and 0.04 mm on an uncapped 600 dpi plot, and a cloud that
/// reads as a surface on screen printed as a faint stipple.
pub const CLOUD_POINT_PAPER_MILLIMETRES: f64 = 0.25;

// ---- the cache ----------------------------------------------------------------------------------

#[derive(Debug)]
struct RasterImage {
    id: ReferenceId,
    image: Image,
}

/// A raster as a page shows it: the part of the image a viewport covers, averaged down to the
/// capped resolution. Kept so that the sheet editor, which paints paper, does not resample it on
/// every repaint - a pan of the editor, or a second viewport over the same image, asks for the
/// same crop again. Least recently used first; bounded as raster_crop_count says.
#[derive(Debug)]
struct RasterCrop {
    id: ReferenceId,
    /// In the raster's pixels.
    source: IntRect,
    /// The resampled image's (columns, rows).
    size: (i32, i32),
    /// Premultiplied.
    image: Image,
}

/// A point cloud's display copy (geometry's point_splat): float offsets, colours, tiles and
/// level-of-detail order, built on the first frame that shows the layer in this colour mode.
#[derive(Debug)]
struct CloudDisplay {
    id: ReferenceId,
    mode: PointColorMode,
    splat: SplatCloud,
}

/// A symbol stamp rasterised once and blitted wherever the same symbol is put in the same pen at
/// the same size and sub-pixel position. The view scale, the paper scale, the device ratio and the
/// library generation are the cache's, not the key's: every sprite goes when one changes. The
/// widths are kept as their bits, so the key orders.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct SpriteKey {
    symbol: String,
    /// With alpha.
    colour: u32,
    pen_width: u64,
    cosmetic: bool,
    /// Style::symbol_size.
    size: u64,
    /// The stamp's sub-pixel position, in quarters of a device pixel.
    phase_x: i32,
    phase_y: i32,
}

#[derive(Debug)]
struct Sprite {
    /// Premultiplied, device pixels, tagged with the device ratio. None: a stamp too large to
    /// cache, remembered as such so it is not measured again at every point.
    image: Option<Image>,
    /// The image's device pixel the stamp's whole pixel lands on.
    anchor: (i32, i32),
}

/// What the painter keeps between frames. Owned by the caller - one per view, one per plotting
/// thread - and never shared between threads. Everything in it is keyed so that a stale entry is
/// not used: flattened definitions on the library generation, raster images and cloud colours on
/// the reference layer's id and mode, a raster's paper crops on its id, the part of it cropped and
/// the size it was resampled to, symbol sprites on the symbol, pen and size.
#[derive(Debug, Default)]
pub struct PlanPaintCache {
    definitions: DefinitionCache,
    /// Plain text's fonts by pixel size, for the family they were made in; on paper one font at a
    /// reference size, scaled to each text's height.
    font_family: String,
    fonts: BTreeMap<i32, Font>,
    paper_font: Option<Font>,
    rasters: Vec<RasterImage>,
    raster_crops: Vec<RasterCrop>,
    clouds: Vec<CloudDisplay>,
    /// The pixels every cloud is splatted into, kept between frames and reallocated only when the
    /// view's size changes: allocating and filling a window-sized image per cloud per frame cost
    /// 2.4 ms a cloud at 1600 x 1000. Each u32 holds a pixel's RGBA bytes in memory order (the
    /// canvas's Image's layout), where the C++'s held a QImage's 0xAARRGGBB; the rows the splat
    /// wrote are copied into `cloud_image` to be drawn.
    cloud_layer: Vec<u32>,
    cloud_layer_size: (i32, i32),
    cloud_image: Image,
    /// Surface display (docs/surface_display.md): each surface's cull grid, joined rim and
    /// breaklines, and traced contours, kept while the drawing has the surface (cad's
    /// SurfacePlanCache).
    surfaces: SurfacePlanCache,
    sprites: BTreeMap<SpriteKey, Sprite>,
    sprite_scale: f64,
    sprite_paper_scale: f64,
    sprite_device_ratio: f64,
    sprite_generation: u64,
}

impl PlanPaintCache {
    /// The paper crops kept: at most MAXIMUM_RASTER_CROPS, and no more than
    /// MAXIMUM_RASTER_CROP_PIXELS between them but for the one last made.
    pub const MAXIMUM_RASTER_CROPS: usize = 8;
    pub const MAXIMUM_RASTER_CROP_PIXELS: usize = 32_000_000;

    pub fn new() -> Self {
        Self::default()
    }

    /// Drops the imagery, its paper crops and the point-cloud colours, which are keyed on a
    /// layer's id and not its contents: call after a reference layer is added, removed, or has
    /// its display settings changed.
    pub fn invalidate_references(&mut self) {
        self.rasters.clear();
        self.raster_crops.clear();
        self.clouds.clear();
    }

    pub fn clear(&mut self) {
        self.definitions.clear();
        self.fonts.clear();
        self.paper_font = None;
        self.rasters.clear();
        self.raster_crops.clear();
        self.clouds.clear();
        self.sprites.clear();
        self.surfaces.clear();
        self.sprite_scale = 0.0;
        self.sprite_paper_scale = 0.0;
        self.sprite_device_ratio = 0.0;
        self.sprite_generation = 0;
    }

    pub fn raster_crop_count(&self) -> usize {
        self.raster_crops.len()
    }

    /// How many distinct fonts plain text has been set in since the last clear, reused rather
    /// than a Font made per text: on screen one per whole pixel size, on paper one for every size
    /// (a paper text is scaled to its exact height rather than rounded to a font size).
    pub fn font_count(&self) -> usize {
        self.fonts.len() + usize::from(self.paper_font.is_some())
    }

    pub fn sprite_count(&self) -> usize {
        self.sprites.len()
    }

    /// What is kept of the surfaces drawn (docs/surface_display.md).
    pub fn surface_cache(&self) -> &SurfacePlanCache {
        &self.surfaces
    }
}

// ---- small helpers ------------------------------------------------------------------------------

const GRID_MINOR: Color = Color::rgb(0x2a, 0x31, 0x39);
const GRID_MAJOR: Color = Color::rgb(0x38, 0x42, 0x4d);
const AXIS: Color = Color::rgb(0x5a, 0x68, 0x75);
/// The one selection colour every view draws (cad's selection_style).
const SELECTION: Color = Color::rgb(SELECTION_RED, SELECTION_GREEN, SELECTION_BLUE);
/// Amber: an overlay, not drawing content.
const ALIGNMENT: Color = Color::rgb(0xff, 0xb7, 0x4d);

/// The size a paper text's one font is made at, in pixels. A text is set by scaling this, so its
/// height on the page is exact to the fraction of a device pixel; the size only has to be large
/// enough that the font's own metrics are not rounded coarsely.
const PAPER_FONT_REFERENCE_PIXELS: i32 = 100;

/// Texts are drawn no taller than this: a font asked for at a size of hundreds of thousands of
/// pixels, one zoom step from a survey's extent, makes the raster engine allocate glyphs larger
/// than any screen.
const MAXIMUM_TEXT_PIXELS: f64 = 2000.0;

/// How far past its entity's box a paper-sized note, leader or dimension may reach, in
/// millimetres on paper: what the index is asked for beyond the view, so a leader whose line is
/// off the view and whose note is on it is still drawn. Each is then culled by its own laid-out
/// extent.
const ANNOTATION_REACH_MILLIMETRES: f64 = 60.0;
/// Lines kept for the label placer to keep text off: past this many in one paint the rest are not
/// collected, and labels may sit on them - a view that shows this much linework shows labels too
/// small to read anyway.
const MAXIMUM_LINEWORK: usize = 200_000;

/// A symbol stamp larger than this across, in device pixels, is stroked and not cached: a sprite
/// of it would cost more memory than it saves time, and a symbol that large is few on screen.
const MAXIMUM_SPRITE_PIXELS: f64 = 256.0;
/// More cached stamps than this and the cache is emptied: a view zoomed through many scales keeps
/// only the current one's anyway (the cache is keyed on the scale), and this bounds a drawing of
/// very many symbol and colour combinations.
const MAXIMUM_SPRITES: usize = 20_000;

/// A text as every text was before styles: no style, no paper height, the baseline-left anchor,
/// one line. Drawn exactly as it always was, by draw_text; anything else goes through the
/// annotation layout.
fn is_plain_text(text: &TextGeometry) -> bool {
    text.style.is_empty()
        && text.paper_height == 0.0
        && text.justify == TextJustify::BottomLeft
        && !text.text.contains('\n')
}

fn canvas_colour(colour: EntityColor) -> Color {
    Color::rgba(colour.r, colour.g, colour.b, colour.a)
}

/// QPen(colour, width): solid, square caps, bevelled joins.
fn pen(colour: Color, width: f64) -> Pen {
    Pen::with(
        colour,
        width,
        PenStyle::Solid,
        PenCap::Square,
        PenJoin::Bevel,
    )
}

/// Whether a mesh is drawn at all - in plan as its footprint, and so counted in what Zoom
/// Extents frames. The 3D scene applies the same rule to it.
fn is_shown(item: &SceneMesh) -> bool {
    item.visible
        && item.mesh.is_some_and(|mesh| !mesh.empty())
        && item.style != SurfaceStyle::Hidden
        // The road model's objects draw their own outlines (draw_road_model).
        && item.plan_footprint
}

fn hidden_reference(frame: &PlanFrame, id: ReferenceId) -> bool {
    frame
        .hidden_references
        .is_some_and(|hidden| hidden.contains(&id))
}

/// Chainage in the civil convention, kilometres + metres: 1234.5 reads as "1+234.50". Formatted
/// as std::to_chars fixed is (format_fixed), never by a locale, which would print "1+234,50" on a
/// machine set to one that uses a decimal comma.
fn format_station(station: f64) -> String {
    let magnitude = station.abs();
    let kilometres = (magnitude / 1000.0) as i64;
    let metres = magnitude - kilometres as f64 * 1000.0;
    let mut metres_text = format_fixed(metres, 2);
    while metres_text.len() < 6 {
        // "000.00"
        metres_text.insert(0, '0');
    }
    format!(
        "{}{kilometres}+{metres_text}",
        if station < 0.0 { "-" } else { "" }
    )
}

/// What a source with no library draws symbols and linestyles from: nothing, so the built-in
/// shapes and plain lines.
fn no_library() -> &'static StyleLibrary {
    static NONE: OnceLock<StyleLibrary> = OnceLock::new();
    NONE.get_or_init(StyleLibrary::default)
}

/// The model a text or leader is resolved against when a preview is drawn with none: it has the
/// Standard text style and nothing else.
fn empty_model() -> &'static Model {
    static EMPTY: OnceLock<Model> = OnceLock::new();
    EMPTY.get_or_init(Model::new)
}

fn no_leader_context() -> &'static LeaderContext {
    static NONE: OnceLock<LeaderContext> = OnceLock::new();
    NONE.get_or_init(LeaderContext::default)
}

fn built_in_styles() -> &'static SurfaceStyleSet {
    static BUILT_IN: OnceLock<SurfaceStyleSet> = OnceLock::new();
    BUILT_IN.get_or_init(built_in_surface_styles)
}

/// The width a pen strokes, in the canvas's units: a cosmetic pen's width is device pixels and
/// never less than one.
fn stroke_width(pen: &Pen) -> f64 {
    if pen.cosmetic || pen.width <= 0.0 {
        katana_math::cpp::max(1.0, pen.width)
    } else {
        pen.width
    }
}

/// QTransform::type() <= TxTranslate: the transform only shifts.
fn merely_shifts(transform: &Transform) -> bool {
    transform.m11 == 1.0 && transform.m22 == 1.0 && transform.m12 == 0.0 && transform.m21 == 0.0
}

/// The frame's viewport corners in the model: for a turned frame the turned rectangle's own
/// corners, where visible_box is the box around them. A device offset (dx, dy) from the centre is
/// where the canvas's rotation by -angle (y down) put the unrotated offset R(angle) (dx, dy) in
/// the same y-down pixels: undo it by turning the other way.
fn viewport_corners(frame: &PlanFrame) -> [Point2; 4] {
    let view = &frame.transform;
    let half_width = 0.5 * view.width_pixels;
    let half_height = 0.5 * view.height_pixels;
    let c = frame.rotation.cos();
    let s = frame.rotation.sin();
    [
        (-half_width, -half_height),
        (half_width, -half_height),
        (half_width, half_height),
        (-half_width, half_height),
    ]
    .map(|(dx, dy)| {
        let ux = c * dx - s * dy;
        let uy = s * dx + c * dy;
        view.screen_to_world(Point2::new(half_width + ux, half_height + uy))
    })
}

/// What one output column (or row) of a resampled image covers of its source: the source pixels
/// [first, first + count), and how much of each, a share of weights starting at `weight`. `total`
/// is the covered length, the sum of those shares.
#[derive(Clone, Copy, Debug, Default)]
struct Footprint {
    first: i32,
    count: i32,
    weight: usize,
    total: f64,
}

/// Splits `source_length` pixels evenly into `output_length`: output pixel o covers source
/// [o L / N, (o + 1) L / N), a pixel the boundary cuts shared between its two neighbours by how
/// much of it each covers.
fn footprints_of(source_length: i32, output_length: i32, weights: &mut Vec<f64>) -> Vec<Footprint> {
    let step = f64::from(source_length) / f64::from(output_length);
    let mut footprints = vec![Footprint::default(); output_length.max(0) as usize];
    for o in 0..output_length {
        let from = f64::from(o) * step;
        let to = if o + 1 == output_length {
            f64::from(source_length)
        } else {
            f64::from(o + 1) * step
        };
        let footprint = &mut footprints[o as usize];
        footprint.first = (from.floor() as i32).clamp(0, source_length - 1);
        let last = ((to.ceil() as i32) - 1).clamp(footprint.first, source_length - 1);
        footprint.count = last - footprint.first + 1;
        footprint.weight = weights.len();
        for s in footprint.first..=last {
            let share = katana_math::cpp::max(
                0.0,
                katana_math::cpp::min(f64::from(s + 1), to)
                    - katana_math::cpp::max(f64::from(s), from),
            );
            weights.push(share);
            footprint.total += share;
        }
    }
    footprints
}

/// The rectangle `crop` of an RGBA image (straight alpha, `stride` bytes a row, top row first - a
/// raster overlay's pixels), averaged down to `size`: each output pixel the mean of the source
/// area it covers, a pixel its edge cuts counted by the part inside (a box filter, exact at any
/// ratio, so a fine image does not shimmer into moire as a point-sampled one does). Premultiplied
/// before it is averaged, so a transparent pixel's colour does not bleed into its neighbours; the
/// result is premultiplied, what the raster backend draws fastest.
///
/// It reads the source in place, a row at a time, and never copies the crop. The output rows are
/// shared out across the pool; each is one task's, summed in the same order at every thread
/// count, so the image is the same whatever the number of threads. Accumulated in f64: a pixel
/// averaging a thousand source pixels square sums a million terms, which an f32 would round to a
/// visibly different colour.
fn area_resample(rgba: &[u8], stride: usize, crop: IntRect, size: (i32, i32)) -> Image {
    let (width, height) = (size.0.max(0) as usize, size.1.max(0) as usize);
    let mut image = Image::new(width as u32, height as u32);
    if width == 0 || height == 0 {
        return image;
    }
    let mut column_weights = Vec::new();
    let mut row_weights = Vec::new();
    let columns = footprints_of(crop.width, size.0, &mut column_weights);
    let rows = footprints_of(crop.height, size.1, &mut row_weights);
    {
        // One lock a row, each taken by the one task that writes that row.
        let out_rows: Vec<Mutex<&mut [u8]>> = image
            .bytes_mut()
            .chunks_mut(width * 4)
            .map(Mutex::new)
            .collect();
        TaskPool::shared().parallel_ranges(0, height, 8, |lo, hi| {
            let mut sums = vec![0.0_f64; width * 4];
            for oy in lo..hi {
                sums.fill(0.0);
                let row = &rows[oy];
                for k in 0..row.count {
                    let wy = row_weights[row.weight + k as usize];
                    if wy <= 0.0 {
                        continue;
                    }
                    let line_start =
                        (crop.y + row.first + k) as usize * stride + crop.x as usize * 4;
                    for (ox, column) in columns.iter().enumerate() {
                        let mut r = 0.0;
                        let mut g = 0.0;
                        let mut b = 0.0;
                        let mut a = 0.0;
                        for j in 0..column.count {
                            let at = line_start + (column.first + j) as usize * 4;
                            let pixel = &rgba[at..at + 4];
                            let coverage =
                                column_weights[column.weight + j as usize] * f64::from(pixel[3]);
                            r += coverage * f64::from(pixel[0]);
                            g += coverage * f64::from(pixel[1]);
                            b += coverage * f64::from(pixel[2]);
                            a += coverage;
                        }
                        let sum = &mut sums[ox * 4..ox * 4 + 4];
                        sum[0] += wy * r;
                        sum[1] += wy * g;
                        sum[2] += wy * b;
                        sum[3] += wy * a;
                    }
                }
                let mut target = out_rows[oy].lock().unwrap_or_else(PoisonError::into_inner);
                for (ox, column) in columns.iter().enumerate() {
                    let area = column.total * row.total;
                    let sum = &sums[ox * 4..ox * 4 + 4];
                    // std::lround: half away from zero, as f64::round.
                    let alpha = if area > 0.0 {
                        ((sum[3] / area).round() as i32).clamp(0, 255)
                    } else {
                        0
                    };
                    // A premultiplied channel is at most its alpha; rounding must not lift it past.
                    let channel = |value: f64| {
                        if area > 0.0 {
                            ((value / (255.0 * area)).round() as i32).clamp(0, alpha)
                        } else {
                            0
                        }
                    };
                    target[ox * 4] = channel(sum[0]) as u8;
                    target[ox * 4 + 1] = channel(sum[1]) as u8;
                    target[ox * 4 + 2] = channel(sum[2]) as u8;
                    target[ox * 4 + 3] = alpha as u8;
                }
            }
        });
    }
    image
}

/// An entity's pens depend on its layer, its style and its own colour besides the frame.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct DisplayKey {
    layer: String,
    style: String,
    colour: u32,
    has_colour: bool,
}

fn colour_key(entity: &Entity) -> (u32, bool) {
    match entity.color {
        Some(c) => (
            (u32::from(c.r) << 24)
                | (u32::from(c.g) << 16)
                | (u32::from(c.b) << 8)
                | u32::from(c.a),
            true,
        ),
        None => (0, false),
    }
}

impl DisplayKey {
    fn of(entity: &Entity) -> Self {
        let (colour, has_colour) = colour_key(entity);
        Self {
            layer: entity.layer.clone(),
            style: entity.style.clone(),
            colour,
            has_colour,
        }
    }

    /// Whether `entity` has this key, without building one: consecutive entities mostly share a
    /// layer and style, and comparing two short strings is cheaper than hashing them.
    fn matches(&self, entity: &Entity) -> bool {
        let (colour, has_colour) = colour_key(entity);
        self.layer == entity.layer
            && self.style == entity.style
            && self.colour == colour
            && self.has_colour == has_colour
    }
}

/// Everything an entity's look depends on besides its geometry, worked out once per distinct
/// layer, style and colour in a paint rather than once per entity: the layer walk, resolve_display's
/// table lookups and string copies, the linetype and hatch resolution, the pens and the flattened
/// library linestyle. Measured in the C++ on a 28k-entity survey drawing this was about a sixth of
/// a frame; a drawing has tens to hundreds of such combinations.
struct Resolved<'a> {
    /// Shown, and not hidden in this view.
    layer_drawn: bool,
    display: ResolvedDisplay,
    hatch: Option<&'a HatchPattern>,
    /// A library linestyle, flattened.
    linestyle: Option<Arc<FlatDefinition>>,
    /// The entity's pen WITHOUT a model linetype's dashes.
    entity_pen: Pen,
    /// The pen the plain line is drawn with.
    pen: Pen,
    target: StylePaintTarget<'a>,
    /// A solid hatch's fill on paper: the fill rule of the plot style (cad's paper_fill_colour),
    /// which in monochrome is not the pen's black. None: the pen's.
    paper_fill: Option<Color>,
}

/// What a point symbol stamp is, worked out once per symbol and size in a paint: where it reaches
/// from its insertion point and whether it is too small to draw.
#[derive(Clone, Copy, Debug)]
struct Stamp {
    /// About the insertion point.
    extent: Box2,
    below_detail: bool,
    empty: bool,
}

// ---- the painter --------------------------------------------------------------------------------

/// One paint's worth of state over the caller's arguments. Built on the stack for each
/// paint_plan, so nothing here outlives a frame except what it puts in the cache - which is what
/// makes paint_plan reentrant.
struct PlanPainter<'p> {
    canvas: &'p mut dyn Canvas,
    source: &'p PlanSource<'p>,
    frame: &'p PlanFrame<'p>,
    options: &'p PlanPaintOptions<'p>,
    cache: &'p mut PlanPaintCache,
    library: &'p StyleLibrary,
    view: ViewTransform,
    visible: Box2,
    stats: PlanPaintStats,

    // The dimension style and hatch pattern in force for the entity being drawn: resolved once
    // per entity, and held here because draw_geometry is handed only the geometry. The hatch
    // pattern is owned by the model, which outlives the paint.
    dimension_style: DimensionStyle,
    hatch: Option<&'p HatchPattern>,
    hatch_paper_fill: Option<Color>,

    // This paint's resolutions, and the last one asked for.
    resolved: Vec<Rc<Resolved<'p>>>,
    resolved_index: HashMap<DisplayKey, usize>,
    last: Option<(DisplayKey, usize)>,
    // By symbol, then by size: a handful of sizes per symbol at most.
    stamps: BTreeMap<String, Vec<(f64, Stamp)>>,
    // Dash patterns by linetype name and pen width (its bits), for this paint only. A pattern is
    // the linetype's CONTENTS at this scale, and the model's linetypes can be edited between two
    // paints with nothing here to say so; kept across paints by name, a view drew the old dashes
    // until it was zoomed and a second plot printed them.
    dashes: BTreeMap<(String, u64), Vec<f64>>,
    // Whether stamps may come from the sprite cache this paint, and where the canvas's
    // (translation-only) transform and the device ratio put a logical pixel.
    sprites: bool,
    translation: PointF,
    device_ratio: f64,
    // Scratch for clipping and for arcs, reused across entities.
    runs: PolylineRuns,
    // While the ghosts of the selection are drawn (draw_selection_ghosts), stroke_polyline lays a
    // line down as dots (dot_path) instead of stroking it: every other pen draws exactly what it
    // drew before. The dots are kept only inside dot_reach, the view in the canvas's own units
    // and a dot beyond; `dots` is their scratch. Where the canvas only shifts the drawing
    // (dot_device) they are laid in device pixels: dot_width square and dot_pitch apart, both in
    // device pixels then; else in its own units.
    ghost_dots: bool,
    dot_reach: RectF,
    dots: Vec<PointF>,
    dot_device: bool,
    dot_width: f64,
    dot_pitch: f64,
    arc_points: Vec<Point2>,
    // The faces annotation text is set in, and the lines drawn this paint that labels keep out of
    // (collected by draw_entities when there are labels to place).
    annotation_fonts: AnnotationFonts,
    linework: Vec<Segment2>,
    collect_linework: bool,
    // The legends of the surfaces drawn this paint (draw_surfaces).
    surface_legends: Vec<PaintedLegend>,
}

impl<'p> PlanPainter<'p> {
    fn new(
        canvas: &'p mut dyn Canvas,
        source: &'p PlanSource<'p>,
        frame: &'p PlanFrame<'p>,
        options: &'p PlanPaintOptions<'p>,
        cache: &'p mut PlanPaintCache,
    ) -> Self {
        if cache.font_family != options.font_family {
            cache.fonts.clear();
            cache.paper_font = None;
            cache.font_family = options.font_family.clone();
        }
        let metrics = options.raster.as_ref().map(|raster| raster.metrics());
        Self {
            canvas,
            source,
            frame,
            options,
            cache,
            library: source.library.unwrap_or(no_library()),
            view: frame.transform,
            visible: visible_box(frame),
            stats: PlanPaintStats::default(),
            dimension_style: DimensionStyle::default(),
            hatch: None,
            hatch_paper_fill: None,
            resolved: Vec::new(),
            resolved_index: HashMap::new(),
            last: None,
            stamps: BTreeMap::new(),
            dashes: BTreeMap::new(),
            sprites: false,
            translation: PointF::default(),
            device_ratio: 1.0,
            runs: PolylineRuns::default(),
            ghost_dots: false,
            dot_reach: RectF::default(),
            dots: Vec::new(),
            dot_device: false,
            dot_width: GHOST_PEN_PIXELS,
            dot_pitch: GHOST_DOT_PITCH_PIXELS,
            arc_points: Vec::new(),
            annotation_fonts: AnnotationFonts::new(&options.font_family, metrics),
            linework: Vec::new(),
            collect_linework: false,
            surface_legends: Vec::new(),
        }
    }

    fn paper(&self) -> bool {
        self.options.medium == PlanMedium::Paper
    }

    fn to_screen(&self, world: Point2) -> PointF {
        let p = self.view.world_to_screen(world);
        PointF::new(p.x, p.y)
    }

    fn width_pixels(&self) -> i32 {
        self.view.width_pixels as i32
    }

    fn height_pixels(&self) -> i32 {
        self.view.height_pixels as i32
    }

    /// Plots keep their own paper size. Auto on screen uses a millimetre of screen; a fixed
    /// display denominator gives library paper marks a model size instead, without changing the
    /// camera or annotation scale.
    fn paper_scale(&self) -> f64 {
        if !self.paper()
            && let Some(denominator) = self.options.paper_style_scale
        {
            return to_metres(denominator, units::MILLIMETRE);
        }
        self.options.pixels_per_millimetre / katana_math::cpp::max(self.view.scale, 1e-12)
    }

    /// A mark `pixels` wide on screen and `millimetres` wide on paper, in device pixels.
    fn mark_size(&self, pixels: f64, millimetres: f64) -> f64 {
        if self.paper() {
            millimetres * self.options.pixels_per_millimetre
        } else {
            pixels
        }
    }

    /// The plain point mark's half-width, in device pixels.
    fn mark_pixels(&self) -> f64 {
        self.mark_size(POINT_MARKER_PIXELS, POINT_MARKER_PAPER_MILLIMETRES)
    }

    /// The plain point mark's size, in model units at the current scale, so a built-in symbol
    /// with no size of its own stays a mark and not a blob.
    fn plain_mark_half_width(&self) -> f64 {
        self.mark_pixels() / katana_math::cpp::max(self.view.scale, 1e-12)
    }

    /// A built-in symbol shape whose style gives it no size is a paper mark on a plot: the plain
    /// mark's POINT_MARKER_PAPER_MILLIMETRES either side. So a fixed paper style scale sizes it
    /// in the plot's millimetres, as it sizes a library paper symbol. Auto and paper keep
    /// plain_mark_half_width. The plain point cross of a point with no symbol is the screen's own
    /// marker and is not drawn through this.
    fn symbol_fallback_half_width(&self) -> f64 {
        if !self.paper() && self.options.paper_style_scale.is_some() {
            return POINT_MARKER_PAPER_MILLIMETRES * self.paper_scale();
        }
        self.plain_mark_half_width()
    }

    /// On paper, the finest imagery is embedded at as a share of the device's resolution:
    /// raster_dpi_cap over the plot's own, and never more than 1 - imagery is not embedded finer
    /// than the page is printed.
    fn imagery_share(&self) -> f64 {
        let ppmm = self.options.pixels_per_millimetre;
        if !(self.options.raster_dpi_cap > 0.0) || !(ppmm > 0.0) {
            return 1.0;
        }
        katana_math::cpp::min(
            1.0,
            millimetres_to_pixels(1.0, self.options.raster_dpi_cap) / ppmm,
        )
    }

    /// One font per size, kept: in the C++ making a font by family name resolved the family
    /// through the font database, which done for each of thousands of labels a frame was a
    /// measurable part of the frame. On screen a text is a whole number of pixels, the screen's
    /// own resolution.
    fn font_for(&mut self, pixels: f64) -> Font {
        let size = pixels as i32;
        let family = &self.options.font_family;
        self.cache
            .fonts
            .entry(size)
            .or_insert_with(|| Font::family(family, FontSize::Pixels(size)))
            .clone()
    }

    /// One font for every paper text, scaled to each: a font's pixel size is whole, and a 2.5 mm
    /// label at 1 : 1000 and 300 dpi is 29.5 device pixels, which a whole-pixel size set 0.04 mm
    /// short (and at 72 dpi, 0.35 mm).
    fn paper_font(&mut self) -> Font {
        let family = &self.options.font_family;
        self.cache
            .paper_font
            .get_or_insert_with(|| {
                Font::family(family, FontSize::Pixels(PAPER_FONT_REFERENCE_PIXELS))
            })
            .clone()
    }

    fn paint(mut self) -> PlanPaintStats {
        self.canvas.save();
        if self.frame.origin.x != 0.0 || self.frame.origin.y != 0.0 {
            self.canvas
                .translate(self.frame.origin.x, self.frame.origin.y);
        }
        if self.frame.clip {
            self.canvas.set_clip_rect(
                RectF::new(0.0, 0.0, self.view.width_pixels, self.view.height_pixels),
                true,
            );
        }
        if self.frame.rotation != 0.0 {
            // About the viewport's centre, the point the transform puts its centre at. Screen y
            // points down, so a counter-clockwise turn of the drawing is a negative angle to the
            // canvas.
            let centre = PointF::new(0.5 * self.view.width_pixels, 0.5 * self.view.height_pixels);
            self.canvas.translate(centre.x, centre.y);
            self.canvas.rotate(-self.frame.rotation * RAD_TO_DEG);
            self.canvas.translate(-centre.x, -centre.y);
        }
        // A stamp is blitted onto whole device pixels, which only a canvas that merely shifts the
        // drawing keeps: under a turn or a scale it is stroked.
        let transform = self.canvas.world_transform();
        self.sprites = self.options.symbol_sprites
            && !self.paper()
            && merely_shifts(&transform)
            && self.options.raster.is_some();
        self.translation = PointF::new(transform.dx, transform.dy);
        self.device_ratio = self.canvas.device_pixel_ratio();
        let paper_scale = self.paper_scale();
        if self.sprites
            && (self.cache.sprite_scale != self.view.scale
                || self.cache.sprite_paper_scale != paper_scale
                || self.cache.sprite_device_ratio != self.device_ratio
                || self.cache.sprite_generation != self.source.library_generation
                || self.cache.sprites.len() > MAXIMUM_SPRITES)
        {
            self.cache.sprites.clear();
            self.cache.sprite_scale = self.view.scale;
            self.cache.sprite_paper_scale = paper_scale;
            self.cache.sprite_device_ratio = self.device_ratio;
            self.cache.sprite_generation = self.source.library_generation;
        }

        // Imagery sits beneath everything: it is a backdrop, and the grid has to stay legible
        // over it. Point clouds sit above the grid but below the drawing, so drawn geometry is
        // never obscured by survey returns. The same order on paper, less the grid: a sheet over
        // an orthophoto prints the photo, the cloud over it and the linework over both.
        if self.options.rasters {
            self.draw_rasters();
        }
        if self.options.grid && !self.paper() {
            self.draw_grid();
        }
        if self.options.point_clouds {
            self.draw_point_clouds();
        }
        self.canvas.set_render_hint(RenderHint::Antialiasing, true);
        // Beneath the drawing, like a surface: a mesh is context for what is drawn over it, not a
        // thing to be picked in plan.
        if self.options.mesh_footprints {
            self.draw_mesh_footprints();
        }
        // Surface display (docs/surface_display.md): beneath the entities, over the imagery and
        // the mesh footprints, as context for what is drawn.
        if self.options.surfaces {
            self.draw_surfaces();
        }
        self.draw_road_model(); // beneath the drawing as a footprint is
        if self.source.model.is_some() {
            self.draw_entities();
            self.draw_selection_ghosts();
            if self.options.labels {
                self.draw_labels();
            }
            if self.options.alignments {
                self.draw_alignments();
            }
        }
        if self.options.surface_legends && !self.paper() {
            self.draw_surface_legends();
        }
        self.canvas.restore();
        self.stats
    }

    // ---- surface display (docs/surface_display.md) ----

    fn draw_surfaces(&mut self) {
        // An empty store is still painted: the cache lets go of what it kept.
        let Some(surfaces) = self.source.surfaces else {
            return;
        };
        let styles = self.source.surface_styles.unwrap_or(built_in_styles());
        let surface = SurfacePaintOptions {
            paper: self.paper(),
            pixels_per_millimetre: self.options.pixels_per_millimetre,
            device_ratio: self.device_ratio,
            imagery_share: self.imagery_share(),
            image_pixel_cap: self.options.raster_pixel_cap,
            plot: self.options.plot,
            rotation: self.frame.rotation,
            font_family: self.options.font_family.clone(),
            halo: if self.paper() {
                Color::WHITE
            } else {
                self.options.screen_background
            },
        };
        let drawn = paint_surfaces_in_plan(
            &mut *self.canvas,
            surfaces,
            styles,
            &self.view,
            &self.visible,
            &surface,
            &mut self.cache.surfaces,
        );
        self.stats.surfaces_drawn += drawn.surfaces_drawn;
        self.stats.surface_triangles_in_view += drawn.triangles_in_view;
        self.stats.surface_edges_stroked += drawn.edges_stroked;
        self.stats.surface_triangles_tinted += drawn.triangles_tinted;
        self.stats.surface_contour_runs += drawn.contour_runs;
        self.stats.surface_labels += drawn.labels;
        self.stats.surface_image_pixels += drawn.image_pixels;
        self.stats.surface_pixels_covered += drawn.pixels_covered;
        self.stats.surface_problems.extend(drawn.problems);
        self.surface_legends = drawn.legends;
    }

    fn draw_surface_legends(&mut self) {
        // Screen furniture in the view's own pixels: only where the frame is not turned (the plan
        // view never is; a turned frame is a sheet's, on paper).
        if self.surface_legends.is_empty() || self.frame.rotation != 0.0 {
            return;
        }
        // Three at most, stacked down from the top-right corner: more would cover the drawing
        // they explain.
        const MAXIMUM_LEGENDS: usize = 3;
        const MARGIN: f64 = 10.0;
        // On the overlay's ground, as a caption or the prompt band is, mostly opaque, in the
        // chrome's text colour: a legend is the view's furniture. The one stack both views draw
        // their legends through (paint_surface_legend_stack): measured before drawn, so a key
        // with its lowest rows cut off by the view's edge never is; a later one that does not
        // fit below the others is left out, and not counted.
        let stack = LegendStack {
            top_right: PointF::new(self.view.width_pixels - MARGIN, MARGIN),
            bottom: self.view.height_pixels - MARGIN,
            gap: MARGIN,
            maximum: MAXIMUM_LEGENDS,
            font: Font::family(&self.options.font_family, FontSize::Pixels(11)),
            text: palette::text(),
            backing: overlay::ground().with_alpha(215),
        };
        self.stats.surface_legends +=
            paint_surface_legend_stack(&mut *self.canvas, &self.surface_legends, &stack).len();
    }

    // ---- imagery ----

    fn draw_rasters(&mut self) {
        let Some(reference) = self.source.reference else {
            return;
        };
        for raster in reference.rasters() {
            // Hidden in the Reference Data panel, or in this view only.
            if !raster.visible
                || hidden_reference(self.frame, raster.id)
                || raster.width <= 0
                || raster.height <= 0
            {
                continue;
            }

            // Pixel -> world is the geotransform; world -> screen is the view. The composition is
            // itself affine, so it is handed to the canvas as one transform rather than
            // resampling the image here.
            //
            //   world.x = g0 + px*g1 + py*g2       screen.x = w/2 + (world.x - cx)*s
            //   world.y = g3 + px*g4 + py*g5       screen.y = h/2 - (world.y - cy)*s
            //
            // so screen.x = [w/2 + (g0-cx)s] + px*(g1 s) + py*(g2 s)
            //    screen.y = [h/2 - (g3-cy)s] + px*(-g4 s) + py*(-g5 s)
            let g = &raster.geotransform;
            let s = self.view.scale;
            let dx = 0.5 * f64::from(self.width_pixels()) + (g[0] - self.view.center.x) * s;
            let dy = 0.5 * f64::from(self.height_pixels()) - (g[3] - self.view.center.y) * s;
            let transform = Transform::new(g[1] * s, -g[4] * s, g[2] * s, -g[5] * s, dx, dy);

            if self.paper() {
                self.draw_raster_on_paper(raster, &transform);
                continue;
            }

            // Cache the image: rebuilding it from the RGBA bytes every frame would copy tens of
            // megabytes per repaint.
            let cached = match self
                .cache
                .rasters
                .iter()
                .position(|entry| entry.id == raster.id)
            {
                Some(at) => at,
                None => {
                    // A raster whose bytes are not its size has no image to draw (the C++ would
                    // read past them).
                    let Ok(image) = Image::from_straight_rgba8(
                        raster.width as u32,
                        raster.height as u32,
                        raster.rgba.clone(),
                    ) else {
                        continue;
                    };
                    self.cache.rasters.push(RasterImage {
                        id: raster.id,
                        image,
                    });
                    // Drawn in this same paint: an imported image must not wait for the next
                    // one, which nothing asks for.
                    self.cache.rasters.len() - 1
                }
            };

            self.canvas.save();
            self.canvas
                .set_opacity(katana_math::cpp::clamp(raster.opacity, 0.0, 1.0));
            // Composed with the frame's own placement (origin, rotation), which is identity for a
            // plain screen view.
            self.canvas.set_world_transform(transform, true);
            // Smooth only when magnifying past 1:1; downsampling a huge image with smoothing on
            // is slow and makes little visible difference.
            self.canvas
                .set_render_hint(RenderHint::SmoothPixmapTransform, (g[1] * s).abs() > 1.0);
            self.canvas
                .draw_image_at(PointF::new(0.0, 0.0), &self.cache.rasters[cached].image);
            self.canvas.restore();
            self.stats.rasters_drawn += 1;
        }
    }

    /// A raster on paper: cropped to the viewport, averaged down to the capped resolution, and
    /// drawn where the whole image would have been. `image_to_view` is the whole image's pixels
    /// to the frame's.
    fn draw_raster_on_paper(&mut self, raster: &RasterOverlay, image_to_view: &Transform) {
        // The screen draws the whole image and lets the view clip it, which on paper would embed
        // all of it - every pixel of a 400-megapixel orthophoto in every PDF page that shows a
        // corner of it - at its own resolution, finer than any plot can print.
        let g = &raster.geotransform;
        let determinant = g[1] * g[5] - g[2] * g[4];
        let bytes_per_row = raster.width as usize * 4;
        if !determinant.is_finite()
            || determinant == 0.0
            || raster.rgba.len() < bytes_per_row * raster.height as usize
        {
            return;
        }

        // The viewport's corners in the image's pixels, through the inverse of the geotransform:
        //   px = ( g5 (x - g0) - g2 (y - g3)) / det
        //   py = (-g4 (x - g0) + g1 (y - g3)) / det
        // The turned rectangle's own corners and not the box around them, so a rotated image
        // under a turned viewport is not cropped to a box around a box.
        let mut left = f64::INFINITY;
        let mut right = f64::NEG_INFINITY;
        let mut top = f64::INFINITY;
        let mut bottom = f64::NEG_INFINITY;
        for corner in viewport_corners(self.frame) {
            let east = corner.x - g[0];
            let north = corner.y - g[3];
            let px = (g[5] * east - g[2] * north) / determinant;
            let py = (g[1] * north - g[4] * east) / determinant;
            left = katana_math::cpp::min(left, px);
            right = katana_math::cpp::max(right, px);
            top = katana_math::cpp::min(top, py);
            bottom = katana_math::cpp::max(bottom, py);
        }
        if !left.is_finite() || !right.is_finite() || !top.is_finite() || !bottom.is_finite() {
            return;
        }
        // A whole pixel of margin beyond the pixels the viewport cuts: a pixel on its edge is
        // kept whole, and the smoothing that draws the crop reads, at the edge, the same
        // neighbours the whole image's would.
        let within =
            |value: f64, limit: i32| katana_math::cpp::clamp(value, 0.0, f64::from(limit)) as i32;
        let x0 = within(left.floor() - 1.0, raster.width);
        let x1 = within(right.ceil() + 1.0, raster.width);
        let y0 = within(top.floor() - 1.0, raster.height);
        let y1 = within(bottom.ceil() + 1.0, raster.height);
        if x1 <= x0 || y1 <= y0 {
            return; // none of it in the viewport
        }
        let crop = IntRect::new(x0, y0, x1 - x0, y1 - y0);

        // How finely to embed it. A step along an image row moves (g1, g4) in the model and a step
        // down a column (g2, g5), so one image pixel reaches |(g1, g4)| s device pixels across and
        // |(g2, g5)| s down, whatever the rotation; in capped pixels, those times imagery_share.
        // Where a pixel reaches less than one capped pixel the crop is averaged down so that it
        // reaches one; where it reaches more it is kept as it is - never enlarged, since a PDF
        // viewer magnifies an embedded image itself. Then the pixel cap, evenly.
        let share = self.imagery_share();
        let across = g[1].hypot(g[4]) * self.view.scale * share;
        let down = g[2].hypot(g[5]) * self.view.scale * share;
        let mut columns = ((f64::from(crop.width) * katana_math::cpp::min(1.0, across)).round()
            as i32)
            .clamp(1, crop.width);
        let mut rows = ((f64::from(crop.height) * katana_math::cpp::min(1.0, down)).round() as i32)
            .clamp(1, crop.height);
        let budget = self.options.raster_pixel_cap.max(1) as f64;
        let pixels = f64::from(columns) * f64::from(rows);
        if pixels > budget {
            // Rounded down, so the product is within the cap.
            let shrink = (budget / pixels).sqrt();
            columns = 1.max((f64::from(columns) * shrink) as i32);
            rows = 1.max((f64::from(rows) * shrink) as i32);
        }
        let size = (columns, rows);

        // The crop, from the cache when the same part of the same raster was resampled to the same
        // size before; most recently used last.
        let crops = &mut self.cache.raster_crops;
        match crops
            .iter()
            .position(|entry| entry.id == raster.id && entry.source == crop && entry.size == size)
        {
            Some(found) => crops[found..].rotate_left(1),
            None => {
                let image = area_resample(&raster.rgba, bytes_per_row, crop, size);
                if image.is_null() {
                    return; // could not be made: the page goes without it
                }
                crops.push(RasterCrop {
                    id: raster.id,
                    source: crop,
                    size,
                    image,
                });
                self.stats.raster_crops_made += 1;
                // Bounded, oldest out first, but never the one just made: a sheet's viewports each
                // want their own crop, and one kept past the bound is at most one capped image.
                let area = |entry: &RasterCrop| entry.size.0 as usize * entry.size.1 as usize;
                let mut total: usize = crops.iter().map(area).sum();
                while crops.len() > 1
                    && (crops.len() > PlanPaintCache::MAXIMUM_RASTER_CROPS
                        || total > PlanPaintCache::MAXIMUM_RASTER_CROP_PIXELS)
                {
                    total -= area(&crops[0]);
                    crops.remove(0);
                }
            }
        }

        // Resampled pixel (u, v) is the crop's pixel (x0 + u w / columns, y0 + v h / rows) - its
        // whole rectangle, exactly - and that is where the whole image's transform puts it.
        let placed = Transform::from_scale(
            f64::from(crop.width) / f64::from(columns),
            f64::from(crop.height) / f64::from(rows),
        ) * Transform::from_translate(f64::from(crop.x), f64::from(crop.y))
            * *image_to_view;
        self.canvas.save();
        self.canvas
            .set_opacity(katana_math::cpp::clamp(raster.opacity, 0.0, 1.0));
        self.canvas.set_world_transform(placed, true);
        // Smoothed: a crop is at most the capped resolution, so it is usually magnified onto the
        // device, where nearest-neighbour would print blocks.
        self.canvas
            .set_render_hint(RenderHint::SmoothPixmapTransform, true);
        // In the plot style: a greyscale or monochrome plot prints the photo grey (plot_image), as
        // it prints every line and fill of the drawing over it. The crop is converted, never the
        // cached whole image.
        let Some(last) = self.cache.raster_crops.last() else {
            self.canvas.restore();
            return;
        };
        match self.options.plot {
            Some(plot) if plot.colour_mode != PlotColourMode::Colour => {
                self.canvas
                    .draw_image_at(PointF::new(0.0, 0.0), &plot_image(&last.image, plot));
            }
            _ => self
                .canvas
                .draw_image_at(PointF::new(0.0, 0.0), &last.image),
        }
        self.canvas.restore();
        self.stats.rasters_drawn += 1;
        self.stats.raster_pixels_embedded += columns as usize * rows as usize;
    }

    fn draw_point_clouds(&mut self) {
        let width = self.width_pixels();
        let height = self.height_pixels();
        let Some(reference) = self.source.reference else {
            return;
        };
        if width <= 0 || height <= 0 {
            return;
        }

        for cloud in reference.point_clouds() {
            // Hidden in the Reference Data panel, or in this view only.
            if !cloud.visible || hidden_reference(self.frame, cloud.id) || cloud.points.is_empty() {
                continue;
            }

            // The display copy depends only on the layer's points and colour mode, so it is built
            // once; only the projection is redone per frame. (A layer whose points were replaced
            // under the same id is caught by its count; invalidate_references() is the rule for
            // anything else.)
            let cached = match self.cache.clouds.iter().position(|entry| {
                entry.id == cloud.id
                    && entry.mode == cloud.color_mode
                    && entry.splat.source_count == cloud.points.len()
            }) {
                Some(at) => at,
                None => {
                    // Drop any stale entry for this layer whose mode has changed.
                    self.cache.clouds.retain(|entry| entry.id != cloud.id);
                    self.cache.clouds.push(CloudDisplay {
                        id: cloud.id,
                        mode: cloud.color_mode,
                        splat: display_copy(cloud),
                    });
                    self.cache.clouds.len() - 1
                }
            };

            if self.paper() {
                self.splat_on_paper(cloud, cached);
                continue;
            }

            // Splat into an image rather than drawing per point: a point drawn by the canvas
            // costs microseconds, which at two million points is seconds per frame. The pixels are
            // the cache's, reused frame to frame; the splat clears and fills only the rows the
            // cloud can reach, in bands across the task pool, and only those rows are drawn.
            if self.cache.cloud_layer_size != (width, height) {
                self.cache.cloud_layer = vec![0; width as usize * height as usize];
                self.cache.cloud_image = Image::new(width as u32, height as u32);
                self.cache.cloud_layer_size = (width, height);
            }
            let view = SplatView {
                centre: self.view.center,
                scale: self.view.scale,
                width,
                height,
                radius: 0.max(cloud.point_size as i32 - 1),
                point_budget: self.options.cloud_point_budget,
            };
            // Opaque colours: a written pixel is valid premultiplied RGBA and an unwritten one is
            // transparent.
            let splat = splat_cloud(
                &self.cache.clouds[cached].splat,
                &view,
                &mut self.cache.cloud_layer,
                width as usize,
                TaskPool::shared(),
            );
            self.stats.cloud_points_in_view += splat.points_in_view;
            self.stats.cloud_points_drawn += splat.points_drawn;
            if splat.row_end > splat.row_begin {
                let from = splat.row_begin as usize * width as usize;
                let to = splat.row_end as usize * width as usize;
                copy_rows(
                    &self.cache.cloud_layer[from..to],
                    &mut self.cache.cloud_image,
                    from,
                );
                let rows = IntRect::new(0, splat.row_begin, width, splat.row_end - splat.row_begin);
                self.canvas
                    .draw_image(rows.to_rect_f(), &self.cache.cloud_image, rows);
            }
        }
    }

    /// A cloud on paper: splatted into an image of the part of the viewport it covers, at the
    /// capped resolution, and that image laid on the page.
    fn splat_on_paper(&mut self, cloud: &PointCloudLayer, cached: usize) {
        // The screen's splat fills an image the size of the view in device pixels: on an A1 page
        // at 300 dpi, 70 megapixels a cloud. On paper the image covers only the part of the
        // viewport the cloud reaches - the box around the turned viewport, so a twisted sheet's
        // corners are not left empty - at the capped resolution.
        let bounds = cloud.world_bounds();
        let area = Box2::new(
            Point2::new(
                katana_math::cpp::max(self.visible.min.x, bounds.min.x),
                katana_math::cpp::max(self.visible.min.y, bounds.min.y),
            ),
            Point2::new(
                katana_math::cpp::min(self.visible.max.x, bounds.max.x),
                katana_math::cpp::min(self.visible.max.y, bounds.max.y),
            ),
        );
        if area.empty() || !(self.view.scale > 0.0) {
            return;
        }
        // A point is a square of 2 radius + 1 pixels of the image, as on screen it is of the
        // screen, and an image pixel is at most CLOUD_POINT_PAPER_MILLIMETRES of paper, so a point
        // is as large on the page at 150 dpi as at 600. The image is grown by a point and a pixel
        // all round, so a point on the area's edge is drawn whole.
        let radius = 0.max(cloud.point_size as i32 - 1);
        let margin = 2.0 * f64::from(radius + 1);
        let share = katana_math::cpp::min(
            self.imagery_share(),
            1.0 / (CLOUD_POINT_PAPER_MILLIMETRES * self.options.pixels_per_millimetre),
        );
        let mut scale = self.view.scale * share; // image pixels a model unit
        let budget = self.options.raster_pixel_cap.max(1) as f64;
        let columns_at = |s: f64| (area.width() * s + margin).ceil();
        let rows_at = |s: f64| (area.height() * s + margin).ceil();
        if columns_at(scale) * rows_at(scale) > budget {
            // Shrunk to the pixel cap. The margin does not shrink with the scale, so the scale is
            // solved for rather than cut by the square root of the excess: (w s + m + 1)(h s + m +
            // 1) = budget, the 1 for the rounding up, is w h s^2 + (w + h)(m + 1) s + (m + 1)^2 -
            // budget = 0.
            let w = area.width();
            let h = area.height();
            let m = margin + 1.0;
            let a = w * h;
            let b = (w + h) * m;
            let c = m * m - budget;
            let mut fitted = 0.0;
            if c < 0.0 {
                fitted = if a > 0.0 {
                    (-b + (b * b - 4.0 * a * c).sqrt()) / (2.0 * a)
                } else if b > 0.0 {
                    -c / b
                } else {
                    scale
                };
            }
            scale = katana_math::cpp::min(scale, fitted * (1.0 - 1e-9));
        }
        let columns = columns_at(scale);
        let rows = rows_at(scale);
        if !(scale > 0.0) || columns * rows > budget {
            return; // a cap too small for even the margin
        }
        let (image_width, image_height) = (columns as i32, rows as i32);
        let mut pixels = vec![0_u32; image_width as usize * image_height as usize];
        let view = SplatView {
            centre: area.center(),
            scale,
            width: image_width,
            height: image_height,
            radius,
            point_budget: self.options.cloud_point_budget,
        };
        let splatted = splat_cloud(
            &self.cache.clouds[cached].splat,
            &view,
            &mut pixels,
            image_width as usize,
            TaskPool::shared(),
        );
        self.stats.cloud_points_in_view += splatted.points_in_view;
        self.stats.cloud_points_drawn += splatted.points_drawn;
        if splatted.row_end <= splatted.row_begin {
            return;
        }
        // Image pixel (u, v) is the model point centre + ((u - W/2) / scale, -(v - H/2) / scale),
        // which the view puts at to_screen(centre) + (u - W/2, v - H/2) k with k = view scale /
        // scale: one scale and a shift, turned with the rest of the frame by the canvas.
        let k = self.view.scale / scale;
        let at = self.to_screen(area.center());
        let placed = Transform::new(
            k,
            0.0,
            0.0,
            k,
            at.x - 0.5 * f64::from(image_width) * k,
            at.y - 0.5 * f64::from(image_height) * k,
        );
        // Only the rows the splat cleared and drew: the rest of the image was never written.
        let band = IntRect::new(
            0,
            splatted.row_begin,
            image_width,
            splatted.row_end - splatted.row_begin,
        );
        let mut image = Image::new(image_width as u32, image_height as u32);
        let from = splatted.row_begin as usize * image_width as usize;
        let to = splatted.row_end as usize * image_width as usize;
        copy_rows(&pixels[from..to], &mut image, from);
        self.canvas.save();
        self.canvas.set_world_transform(placed, true);
        // Not smoothed: a point is a crisp square, as on screen.
        self.canvas
            .set_render_hint(RenderHint::SmoothPixmapTransform, false);
        // The points in the plot style, as the crop of a raster is.
        match self.options.plot {
            Some(plot) if plot.colour_mode != PlotColourMode::Colour => {
                let grey = plot_image(&image.copy(band), plot);
                self.canvas
                    .draw_image_at(PointF::new(f64::from(band.x), f64::from(band.y)), &grey);
            }
            _ => self.canvas.draw_image(band.to_rect_f(), &image, band),
        }
        self.canvas.restore();
        self.stats.cloud_pixels_embedded += band.width as usize * band.height as usize;
    }

    fn draw_grid(&mut self) {
        let spacing = grid_spacing(self.view.scale, DEFAULT_GRID_PIXELS);
        let first_index = |lo: f64| (lo / spacing).floor() as i64;
        let last_index = |hi: f64| (hi / spacing).ceil() as i64;
        let width = f64::from(self.width_pixels());
        let height = f64::from(self.height_pixels());

        // Every fifth line is a major line so that distances can be read off.
        for i in first_index(self.visible.min.x)..=last_index(self.visible.max.x) {
            self.canvas
                .set_pen(pen(if i % 5 == 0 { GRID_MAJOR } else { GRID_MINOR }, 1.0));
            let x = self.to_screen(Point2::new(i as f64 * spacing, 0.0)).x;
            self.canvas
                .draw_line(PointF::new(x, 0.0), PointF::new(x, height));
        }
        for i in first_index(self.visible.min.y)..=last_index(self.visible.max.y) {
            self.canvas
                .set_pen(pen(if i % 5 == 0 { GRID_MAJOR } else { GRID_MINOR }, 1.0));
            let y = self.to_screen(Point2::new(0.0, i as f64 * spacing)).y;
            self.canvas
                .draw_line(PointF::new(0.0, y), PointF::new(width, y));
        }
        let origin = self.to_screen(Point2::new(0.0, 0.0));
        self.canvas.set_pen(pen(AXIS, 1.0));
        self.canvas
            .draw_line(PointF::new(origin.x, 0.0), PointF::new(origin.x, height));
        self.canvas
            .draw_line(PointF::new(0.0, origin.y), PointF::new(width, origin.y));
    }

    /// A mesh in plan is its FOOTPRINT - the hull of its vertices - and not its triangles: 1 453
    /// meshes of 90 656 triangles arrive from one real archive, and drawing those in plan would
    /// bury the drawing they are context for. The 3D view is where a mesh is looked at.
    fn draw_mesh_footprints(&mut self) {
        let Some(meshes) = self.source.meshes else {
            return;
        };
        for item in meshes {
            if !is_shown(item) {
                continue;
            }
            let Some(mesh) = item.mesh else {
                continue;
            };
            let hull = mesh.plan_hull();
            if hull.len() < 2 {
                continue;
            }
            let mut flat = EntityColor {
                r: red_of(item.flat_color),
                g: green_of(item.flat_color),
                b: blue_of(item.flat_color),
                a: 255,
            };
            if self.paper()
                && let Some(plot) = self.options.plot
            {
                flat = paper_colour(flat, plot); // a white mesh prints black
            }
            let colour = canvas_colour(flat);
            let outline = colour.with_alpha(190);
            if self.paper() {
                // A pen of paper millimetres, dashed in millimetres: the screen's one-pixel pen
                // would be a hairline on a 600 dpi plot and twice as heavy at 300, and a dash is
                // counted in pen widths, so the screen's dash at 0.13 mm would be dots half a
                // millimetre long.
                self.canvas.set_pen(Pen::with(
                    outline,
                    self.mark_size(1.0, MESH_OUTLINE_PAPER_MILLIMETRES),
                    PenStyle::Custom(vec![
                        MESH_DASH_PAPER_MILLIMETRES / MESH_OUTLINE_PAPER_MILLIMETRES,
                        MESH_GAP_PAPER_MILLIMETRES / MESH_OUTLINE_PAPER_MILLIMETRES,
                    ]),
                    PenCap::Flat,
                    PenJoin::Bevel,
                ));
            } else {
                self.canvas.set_pen(Pen::with(
                    outline,
                    1.0,
                    PenStyle::Dash,
                    PenCap::Square,
                    PenJoin::Bevel,
                ));
            }
            let polygon: Vec<PointF> = hull.iter().map(|&vertex| self.to_screen(vertex)).collect();
            // Two points are a wall seen from above: a line, with nothing to fill.
            if hull.len() == 2 {
                self.canvas.draw_polyline(&polygon);
                continue;
            }
            self.canvas.set_brush(Brush::solid(colour.with_alpha(40)));
            self.canvas.draw_polygon(&polygon, FillRule::OddEven);
            self.canvas.set_brush(Brush::none());
        }
    }

    /// The road model in plan (docs/road_design.md): each object shown (bim_view's is_shown, with
    /// this view's hidden layers) as its outline in its own colour, beneath the drawing, as a
    /// mesh's footprint is - the road model is context for what is drawn over it. Earthworks
    /// dashed, being the ground's and not the road's. Its pen is a mesh footprint's, on screen and
    /// on paper.
    fn draw_road_model(&mut self) {
        let (Some(road_model), Some(model)) = (self.source.road_model.clone(), self.source.model)
        else {
            return;
        };
        let overrides = self.frame.layers.unwrap_or(&NO_LAYER_OVERRIDES);
        for object in &road_model.objects {
            if object.plan.is_empty() || !bim_view::is_shown(object, model, overrides) {
                continue;
            }
            let mut colour = object.colour;
            if self.paper()
                && let Some(plot) = self.options.plot
            {
                colour = paper_colour(colour, plot);
            }
            let earthworks = object.kind == "earthworks";
            let mut outline = pen(
                canvas_colour(colour),
                self.mark_size(1.0, MESH_OUTLINE_PAPER_MILLIMETRES),
            );
            if earthworks {
                outline.style = PenStyle::Dash;
            }
            self.canvas.set_pen(outline);
            let fill = canvas_colour(colour).with_alpha(30);
            for line in &object.plan {
                let bounds = line.bounding_box();
                if bounds.empty()
                    || bounds.max.x < self.visible.min.x
                    || bounds.min.x > self.visible.max.x
                    || bounds.max.y < self.visible.min.y
                    || bounds.min.y > self.visible.max.y
                {
                    continue;
                }
                let polygon: Vec<PointF> = line
                    .vertices
                    .iter()
                    .map(|&vertex| self.to_screen(vertex))
                    .collect();
                if line.closed {
                    self.canvas.set_brush(if earthworks {
                        Brush::none()
                    } else {
                        Brush::solid(fill)
                    });
                    self.canvas.draw_polygon(&polygon, FillRule::OddEven);
                    self.canvas.set_brush(Brush::none());
                } else {
                    self.canvas.draw_polyline(&polygon);
                }
            }
        }
    }

    // ---- entities ----

    fn resolve(&mut self, entity: &Entity) -> Rc<Resolved<'p>> {
        if let Some((key, index)) = &self.last
            && key.matches(entity)
        {
            return Rc::clone(&self.resolved[*index]);
        }
        let key = DisplayKey::of(entity);
        let index = match self.resolved_index.get(&key) {
            Some(&index) => index,
            None => {
                let resolved = self.resolve_new(entity);
                self.resolved.push(Rc::new(resolved));
                self.resolved_index
                    .insert(key.clone(), self.resolved.len() - 1);
                self.stats.displays_resolved += 1;
                self.resolved.len() - 1
            }
        };
        self.last = Some((key, index));
        Rc::clone(&self.resolved[index])
    }

    fn resolve_new(&mut self, entity: &Entity) -> Resolved<'p> {
        let model: &'p Model = self.source.model.unwrap_or(empty_model());
        let overrides = self.frame.layers.unwrap_or(&NO_LAYER_OVERRIDES);
        // The layer is resolved ONCE and the visibility rule asked about that (cad's is_drawn
        // without the entity's own flag, which the caller adds), rather than looked up again
        // inside is_drawn.
        let layer = model.layers.resolve(&entity.layer);
        let layer_drawn = layer.shown && !overrides.hides(&entity.layer);
        // Through the one resolution chain, so this agrees with the 3D view and so that a named
        // style can change how an entity looks.
        let display = resolve_display(model, entity);
        // One answer to what the linetype NAME draws (decisions D2 and D8): a library linestyle's
        // own strokes, a model linetype's dashes, or a plain line - never a symbol laid along the
        // line as a pattern.
        let pattern = resolve_line_pattern(model, self.library, &display.linetype, &display.symbol);
        // On paper, white and near-white print black (D7): white is a new layer's colour and a
        // third of the reference survey code file's, and it would vanish into the sheet.
        let mut color = canvas_colour(match self.options.plot {
            Some(plot) if self.paper() => paper_colour(display.color, plot),
            _ => display.color,
        });
        // The fading of a locked layer is screen furniture: it says what the user is working on,
        // and a plot printed it in half tone (audit QT-26).
        if !self.paper() && layer.locked {
            color = color.with_alpha(110); // locked layers, and their children, read as background
        }
        // On screen every line is a hairline: a screen has no paper for a line weight to be
        // millimetres of. It is 1.5 px, or with the view's thin-line option a cosmetic pixel. On a
        // plot the width is the line weight - "millimetres on paper" - which means what it says.
        let mut pen_width_pixels: f64 = 1.5;
        let mut paper_fill = None;
        if self.paper() {
            // The plot style's line weight scale, when there is a plot.
            let line_scale = self.options.plot.map_or(1.0, |plot| plot.line_weight_scale);
            pen_width_pixels =
                display.line_weight * line_scale * self.options.pixels_per_millimetre;
            if let Some(plot) = self.options.plot {
                paper_fill = Some(canvas_colour(paper_fill_colour(display.color, plot)));
            }
        } else if self.options.thin_lines {
            pen_width_pixels = 1.0;
        }
        let entity_pen = Pen {
            cosmetic: !self.paper() && self.options.thin_lines,
            ..pen(color, pen_width_pixels)
        };
        let mut line_pen = entity_pen.clone();
        // Dashes are MODEL lengths: a 0.5 m dash stays half a metre of ground at every zoom, so
        // the pixel pattern is recomputed from the view scale. The array is in units of PEN
        // WIDTH, not pixels, which is why the width is passed in rather than assumed. The pattern
        // depends only on the linetype, the view scale and the pen width, so it is built once per
        // paint (dashes). Only a MODEL linetype dashes the pen: a library linestyle of the same
        // name wins, and its strokes are drawn undashed (D2).
        if pattern.kind == LinetypeKind::ModelLinetype
            && let Some(linetype) = pattern.linetype
        {
            let view_scale = self.view.scale;
            let dashes = self
                .dashes
                .entry((display.linetype.clone(), pen_width_pixels.to_bits()))
                .or_insert_with(|| {
                    let options = DashOptions {
                        view_scale,
                        ..DashOptions::default()
                    };
                    qt_dash_pattern(linetype, &options, pen_width_pixels)
                });
            if !dashes.is_empty() {
                line_pen.style = PenStyle::Custom(dashes.clone());
            }
        }
        // A library definition is painted in the entity's colour and width with a FLAT cap, so a
        // 3 mm dash plots 3 mm rather than 3 mm and a pen width (a square cap); the painter draws
        // its dots round.
        let target = StylePaintTarget {
            view: self.view,
            entity_pen: Pen {
                cap: PenCap::Flat,
                ..entity_pen.clone()
            },
            paper: if self.paper() {
                self.options.plot
            } else {
                None
            },
            entity_pen_only: false,
            colours: self.source.colours,
        };
        // The hatch from the display already resolved, rather than a second full
        // resolve_display of its own.
        let hatch = resolve_hatch_pattern_of_display(model, &display);
        let linestyle = match (pattern.kind, pattern.definition) {
            (LinetypeKind::LibraryDefinition, Some(definition)) => self.cache.definitions.find(
                self.library,
                self.source.library_generation,
                &definition.name,
            ),
            _ => None,
        };
        Resolved {
            layer_drawn,
            display,
            hatch,
            linestyle,
            entity_pen,
            pen: line_pen,
            target,
            paper_fill,
        }
    }

    fn stamp_of(&mut self, symbol: &str, size: f64) -> Stamp {
        if let Some(found) = self
            .stamps
            .get(symbol)
            .and_then(|sizes| sizes.iter().find(|entry| entry.0 == size))
        {
            return found.1;
        }
        // At the origin: a stamp's reach and size do not depend on where it is put, and at the
        // origin its strokes are not rounded to the magnitude of a map grid coordinate.
        let paper_scale = self.paper_scale();
        let fallback = self.symbol_fallback_half_width();
        let drawing = point_symbol_drawing_cached(
            &mut self.cache.definitions,
            self.library,
            self.source.library_generation,
            symbol,
            Point2::new(0.0, 0.0),
            size,
            0.0,
            paper_scale,
            fallback,
        );
        let extent = drawn_extent(&drawing);
        let stamp = Stamp {
            extent,
            empty: extent.empty(),
            below_detail: below_symbol_detail(&drawing, self.view.scale, MINIMUM_SYMBOL_PIXELS),
        };
        self.stamps
            .entry(String::from(symbol))
            .or_default()
            .push((size, stamp));
        stamp
    }

    fn draw_entities(&mut self) {
        let Some(model) = self.source.model else {
            return;
        };
        let visible = self.visible;
        let millimetre = self.paper_scale();
        let fallback = self.symbol_fallback_half_width();

        // How far each style's symbol reaches from the point it is put at, at this frame's scale.
        // A symbol is culled by what it DRAWS, not by its insertion point: one library symbol
        // draws 434 m from its point and the plot stamps up to 388 m, and culling on the point
        // dropped every one whose point was just off screen while its strokes were in view. The
        // index is asked for the view grown by the furthest reach, and each entity then by its own
        // style's.
        let mut symbol_reach: BTreeMap<String, f64> = BTreeMap::new();
        let mut furthest_reach: f64 = 0.0;
        {
            let definitions = &mut self.cache.definitions;
            let library = self.library;
            let generation = self.source.library_generation;
            model.styles.for_each(|style| {
                if style.symbol.is_empty() {
                    return;
                }
                let bounds = drawn_extent(&point_symbol_drawing_cached(
                    definitions,
                    library,
                    generation,
                    &style.symbol,
                    Point2::new(0.0, 0.0),
                    style.symbol_size,
                    0.0,
                    millimetre,
                    fallback,
                ));
                if bounds.empty() {
                    return;
                }
                let reach = katana_math::cpp::max(
                    katana_math::cpp::max(bounds.min.x.abs(), bounds.max.x.abs()),
                    katana_math::cpp::max(bounds.min.y.abs(), bounds.max.y.abs()),
                );
                symbol_reach.entry(style.name.clone()).or_insert(reach);
                furthest_reach = katana_math::cpp::max(furthest_reach, reach);
            });
        }

        // Through the spatial index. This runs on EVERY repaint - every pan, every zoom - not just
        // on a click, so it is the scan that mattered most. for_each_candidate falls back to the
        // ordered scan for a zoomed-out repaint, where asking the index for everything would be
        // slower than walking the model once.
        let mut scratch = Vec::new();
        // A paper-sized note or dimension reaches past its entity's box by its size at this scale
        // (docs/annotation.md): the index is asked for that much more, and each is then culled by
        // what it draws.
        let annotation_reach =
            annotation_model_size(ANNOTATION_REACH_MILLIMETRES, self.options.annotation_scale);
        self.collect_linework = self.options.labels
            && self.options.avoid_label_collisions
            && model.label_styles.size() > 0;
        self.linework.clear();
        let index = self.source.index;
        let reach = visible.inflated(katana_math::cpp::max(furthest_reach, annotation_reach));
        for_each_candidate(model, index, reach, &mut scratch, |entity| {
            self.draw_entity(model, entity, &symbol_reach);
        });
    }

    fn draw_entity(
        &mut self,
        model: &'p Model,
        entity: &Entity,
        symbol_reach: &BTreeMap<String, f64>,
    ) {
        // Labels are placed together, after everything else (draw_labels).
        if matches!(entity.geometry, Geometry::Label(_)) {
            return;
        }
        let resolved = self.resolve(entity);
        // cad's is_drawn, with the layer's half answered once per layer.
        if !entity.visible || !resolved.layer_drawn {
            return;
        }
        // Construction lines and rays are drawing aids: on screen, never on paper
        // (drawing/construction).
        if self.paper() && is_construction_layer(&entity.layer) {
            return;
        }
        // This box test is NOT the one for_each_candidate already did: query_extents is
        // deliberately wider than the geometry (an arc offers its centre for snapping), so this
        // is the tighter, drawing-specific filter and removing it would paint entities that are
        // off screen. Except for a dimension, whose DRAWING - label, arrows, extension overshoot
        // - reaches beyond its geometry's box, which holds only the measured points and the
        // dimension line: culling on that box dropped a dimension whose label alone was on screen
        // (audit QT-25). Its query extent is exactly the drawing's box. A style's symbol widens
        // the box by its reach, for the same reason.
        let dimension = matches!(entity.geometry, Geometry::Dimension(_));
        // A styled, justified, multi-line or paper-sized text and a leader are laid out here,
        // once, and culled and drawn from the layout.
        let measure = self.annotation_fonts.measure();
        let annotation: Option<Drawing> = match &entity.geometry {
            Geometry::Text(text) if !is_plain_text(text) => Some(layout_text_entity(
                model,
                text,
                self.options.annotation_scale,
                &measure,
            )),
            Geometry::Leader(leader) => Some(build_leader(
                model,
                leader,
                self.options.annotation_scale,
                &measure,
                self.leader_context(),
            )),
            _ => None,
        };
        let mut dimension_drawing: Option<DimensionDrawing> = None;
        let mut drawn = if let Some(annotation) = &annotation {
            annotation.extent
        } else if let Geometry::Dimension(geometry) = &entity.geometry {
            let built = build_dimension(
                geometry,
                resolve_dimension_style(model, entity),
                self.options.annotation_scale,
            );
            let extent = if built.extent.empty() {
                bounding_box(&entity.geometry)
            } else {
                built.extent
            };
            dimension_drawing = Some(built);
            extent
        } else {
            bounding_box(&entity.geometry)
        };
        if !resolved.display.symbol.is_empty()
            && let Some(&reach) = symbol_reach.get(&entity.style)
        {
            drawn = drawn.inflated(reach);
        }
        if !drawn.intersects(self.visible) {
            return;
        }
        self.stats.entities_drawn += 1;
        let display = &resolved.display;
        // The selection is screen furniture: it says what the user is working on, and a plot of a
        // drawing printed it in orange dashes (audit QT-26).
        let selected = !self.paper()
            && self
                .source
                .selection
                .is_some_and(|selection| selection.contains(entity.id));
        let selected_target;
        let target: &StylePaintTarget<'p> = if selected {
            let selection = Pen::with(
                SELECTION,
                2.0,
                PenStyle::Dash,
                PenCap::Square,
                PenJoin::Bevel,
            );
            self.canvas.set_pen(selection.clone());
            selected_target = StylePaintTarget {
                entity_pen: Pen {
                    cap: PenCap::Flat,
                    ..selection
                },
                entity_pen_only: true,
                ..resolved.target.clone()
            };
            &selected_target
        } else {
            self.canvas.set_pen(resolved.pen.clone());
            &resolved.target
        };
        self.hatch = resolved.hatch;
        self.hatch_paper_fill = resolved.paper_fill;
        // Looked up only for a dimension, the one entity that can use it.
        if dimension {
            self.dimension_style = resolve_dimension_style(model, entity).clone();
        }
        if self.collect_linework {
            // A note's, a callout's and a dimension's text is kept out of as well as the lines
            // (label_layout's append_keep_out).
            append_linework(&entity.geometry, &mut self.linework, MAXIMUM_LINEWORK);
            if let Some(annotation) = &annotation {
                append_keep_out(annotation, &mut self.linework, MAXIMUM_LINEWORK);
            } else if let Some(drawing) = &dimension_drawing {
                append_dimension_keep_out(drawing, &mut self.linework, MAXIMUM_LINEWORK);
            }
        }
        if let Some(annotation) = &annotation {
            self.draw_annotation(annotation);
            return;
        }
        if let Geometry::Point(point) = &entity.geometry
            && !display.symbol.is_empty()
        {
            self.draw_symbol(target, &display.symbol, point.position, display.symbol_size);
            return;
        }
        // A library linestyle IS the line, gaps and all, so it REPLACES the plain line rather
        // than being drawn over it. A pattern too fine to see or too long to lay is the plain
        // line, and so is one that is not laid for any other reason: never nothing.
        let mut drawn_by_style = false;
        if let Some(linestyle) = &resolved.linestyle {
            drawn_by_style = self.draw_line_style(target, linestyle, &entity.geometry);
        }
        // A hatch is painted inside draw_geometry, so an entity carrying one is drawn anyway and
        // puts up with a doubled outline.
        if !drawn_by_style || self.hatch.is_some() {
            self.draw_geometry(&entity.geometry);
        }
        // A line whose style names a symbol carries it at EVERY vertex (D8), as the archive import
        // intends: a fence line's posts, a string of drill holes. The line above is drawn as well.
        if !display.symbol.is_empty() {
            for vertex in symbol_vertices(&entity.geometry) {
                self.draw_symbol(target, &display.symbol, vertex, display.symbol_size);
            }
        }
    }

    /// A linestyle runs along whatever plan shape the entity has. An arc and a circle are chorded
    /// first, because a pattern is laid by distance along a path and a path is what a polyline
    /// is.
    fn draw_line_style(
        &mut self,
        target: &StylePaintTarget,
        definition: &FlatDefinition,
        geometry: &Geometry,
    ) -> bool {
        // Only the repeats that can reach the view are laid, each exactly where it falls on the
        // whole line; a pattern finer than two pixels or longer than the budget is not laid at
        // all, and the caller draws the plain line (audit CAD-04: the tail of a long line used to
        // vanish silently).
        let options = LinestyleOptions {
            paper_scale: self.paper_scale(),
            view_scale: self.view.scale,
            visible: Some(self.visible),
            ..LinestyleOptions::default()
        };
        // A quarter of a pixel, the same accuracy draw_geometry chords to, so a pattern laid along
        // a curve follows the curve that was drawn.
        let chord_tolerance = 0.25 / katana_math::cpp::max(self.view.scale, 1e-12);
        let shape = match geometry {
            Geometry::Line(segment) => Polyline2::new(vec![segment.start, segment.end], false),
            Geometry::Polyline(polyline) => polyline.clone(),
            Geometry::Arc(arc) => Polyline2::new(chord_arc(arc, chord_tolerance), false),
            Geometry::Circle(circle) => Polyline2::new(chord_circle(circle, chord_tolerance), true),
            // A point, a text and a mesh have no line to lay a pattern along.
            _ => return false,
        };
        if shape.vertices.len() < 2 {
            return false;
        }
        let laid = style_drawing(definition, &shape, &options);
        if laid.outcome != LinestyleLayoutOutcome::Laid {
            return false; // too fine, too long or degenerate: the caller draws the plain line
        }
        paint_style_drawing(&mut *self.canvas, &laid.drawing, target);
        true
    }

    fn draw_symbol(&mut self, target: &StylePaintTarget, symbol: &str, centre: Point2, size: f64) {
        // Where the stamp reaches and whether it is too small to draw are the same wherever it is
        // put, so they are worked out once per symbol and size, and a stamp off screen or blitted
        // from a sprite is never built.
        let stamp = self.stamp_of(symbol, size);
        if stamp.empty {
            return; // NO_SYMBOL, or a definition that draws nothing
        }
        let extent = Box2::new(
            Point2::new(stamp.extent.min.x + centre.x, stamp.extent.min.y + centre.y),
            Point2::new(stamp.extent.max.x + centre.x, stamp.extent.max.y + centre.y),
        );
        if !extent.intersects(self.visible) {
            return; // culled by what it draws, not by where it stands
        }
        self.stats.symbols_stamped += 1;
        if stamp.below_detail {
            // Under three pixels a symbol is a smudge: a dot in its pen says a point is there, for
            // one draw call instead of every stroke.
            let mut dot = target.entity_pen.clone();
            dot.cap = PenCap::Round;
            dot.width = katana_math::cpp::max(dot.width, 2.0);
            let previous = self.canvas.pen();
            self.canvas.set_pen(dot);
            let at = self.to_screen(centre);
            self.canvas.draw_point(at);
            self.canvas.set_pen(previous);
            return;
        }
        if self.sprites
            && !target.entity_pen_only
            && self.stamp_sprite(target, symbol, centre, size, &stamp)
        {
            self.stats.sprites_drawn += 1;
            return;
        }
        // Through the one resolver the previews use: a loaded library definition first, the
        // sixteen built-in shapes after. Rotation is 0 because nothing in the model carries one
        // yet.
        let paper_scale = self.paper_scale();
        let fallback = self.symbol_fallback_half_width();
        let drawing = point_symbol_drawing_cached(
            &mut self.cache.definitions,
            self.library,
            self.source.library_generation,
            symbol,
            centre,
            size,
            0.0,
            paper_scale,
            fallback,
        );
        paint_style_drawing(&mut *self.canvas, &drawing, target);
    }

    /// A survey drawing puts the same few symbols in the same few pens at thousands of points;
    /// stroking each from scratch was a third of a frame in the C++ (14 059 stamps, 267 ms, on
    /// the 28k-entity archive). Rasterised once into an image at the stamp's quarter-pixel
    /// position and blitted onto whole device pixels, each stamp after the first is a copy of a
    /// few hundred pixels, and differs from the stroked one by at most an eighth of a pixel of
    /// position.
    fn stamp_sprite(
        &mut self,
        target: &StylePaintTarget,
        symbol: &str,
        centre: Point2,
        size: f64,
        stamp: &Stamp,
    ) -> bool {
        let Some(raster) = self.options.raster.clone() else {
            return false;
        };
        let logical = self.to_screen(centre) + self.translation;
        let device_x = logical.x * self.device_ratio;
        let device_y = logical.y * self.device_ratio;
        let placeable = device_x.abs() < 1.0e6 && device_y.abs() < 1.0e6;
        if !placeable {
            return false; // no whole-pixel position to speak of; stroke it
        }
        // The nearest quarter pixel: a stamp is off by at most an eighth.
        let mut whole_x = device_x.floor();
        let mut whole_y = device_y.floor();
        let mut phase_x = ((device_x - whole_x) * 4.0).round() as i32;
        let mut phase_y = ((device_y - whole_y) * 4.0).round() as i32;
        if phase_x == 4 {
            phase_x = 0;
            whole_x += 1.0;
        }
        if phase_y == 4 {
            phase_y = 0;
            whole_y += 1.0;
        }
        let colour = target.entity_pen.color;
        let key = SpriteKey {
            symbol: String::from(symbol),
            colour: u32::from_be_bytes([colour.a, colour.r, colour.g, colour.b]),
            pen_width: target.entity_pen.width.to_bits(),
            cosmetic: target.entity_pen.cosmetic,
            size: size.to_bits(),
            phase_x,
            phase_y,
        };
        if !self.cache.sprites.contains_key(&key) {
            let mut sprite = Sprite {
                image: None,
                anchor: (0, 0),
            };
            // The stamp's reach in device pixels about its insertion point, plus what the pen and
            // antialiasing add beyond the strokes' own points.
            let ratio = self.device_ratio;
            let s = self.view.scale * ratio;
            let pad = (0.75 * stroke_width(&target.entity_pen) * ratio + 2.0).ceil();
            let left = (-stamp.extent.min.x * s + pad).ceil();
            let right = (stamp.extent.max.x * s + pad).ceil();
            let top = (stamp.extent.max.y * s + pad).ceil(); // y down
            let bottom = (-stamp.extent.min.y * s + pad).ceil();
            let width = left + right + 1.0;
            let height = top + bottom + 1.0;
            if width <= MAXIMUM_SPRITE_PIXELS
                && height <= MAXIMUM_SPRITE_PIXELS
                && width > 0.0
                && height > 0.0
            {
                let mut image = Image::new(width as u32, height as u32);
                image.set_device_pixel_ratio(ratio);
                // A view onto the image that puts the origin - where the stamp is built - at
                // device pixel (left, top) plus the phase, so the strokes land in the image
                // exactly as they would have landed on the device relative to the stamp's whole
                // pixel.
                let mut view = ViewTransform {
                    scale: self.view.scale,
                    width_pixels: width / ratio,
                    height_pixels: height / ratio,
                    ..ViewTransform::default()
                };
                let anchor_x = (left + 0.25 * f64::from(phase_x)) / ratio;
                let anchor_y = (top + 0.25 * f64::from(phase_y)) / ratio;
                view.center = Point2::new(
                    (0.5 * view.width_pixels - anchor_x) / self.view.scale,
                    (anchor_y - 0.5 * view.height_pixels) / self.view.scale,
                );
                let local = StylePaintTarget {
                    view,
                    ..target.clone()
                };
                let paper_scale = self.paper_scale();
                let fallback = self.symbol_fallback_half_width();
                let drawing = point_symbol_drawing_cached(
                    &mut self.cache.definitions,
                    self.library,
                    self.source.library_generation,
                    symbol,
                    Point2::new(0.0, 0.0),
                    size,
                    0.0,
                    paper_scale,
                    fallback,
                );
                let hints = [
                    RenderHint::Antialiasing,
                    RenderHint::TextAntialiasing,
                    RenderHint::SmoothPixmapTransform,
                ]
                .map(|hint| (hint, self.canvas.render_hint(hint)));
                raster.paint_image(&mut image, &mut |canvas: &mut dyn Canvas| {
                    for (hint, on) in hints {
                        canvas.set_render_hint(hint, on);
                    }
                    paint_style_drawing(canvas, &drawing, &local);
                });
                sprite.image = Some(image);
                sprite.anchor = (left as i32, top as i32);
            }
            self.cache.sprites.insert(key.clone(), sprite);
        }
        let Some(sprite) = self.cache.sprites.get(&key) else {
            return false;
        };
        let Some(image) = &sprite.image else {
            return false;
        };
        let at = PointF::new(
            (whole_x - f64::from(sprite.anchor.0)) / self.device_ratio - self.translation.x,
            (whole_y - f64::from(sprite.anchor.1)) / self.device_ratio - self.translation.y,
        );
        self.canvas.draw_image_at(at, image);
        true
    }

    /// The polyline through `vertices` (model units), in the canvas's pen, clipped to the view
    /// when that cannot change a pixel.
    fn stroke_polyline(&mut self, vertices: &[Point2], closed: bool) {
        if self.ghost_dots {
            let path: Vec<PointF> = vertices.iter().map(|&v| self.to_screen(v)).collect();
            self.dot_path(&path, closed);
            return;
        }
        let pen = self.canvas.pen();
        // Only a solid pen is clipped: a dash pattern starts at the start of the line, so a line
        // cut at the view's edge would start its dashes there and every dash would move.
        let clip = self.options.clip_lines
            && !self.paper()
            && pen.style == PenStyle::Solid
            && vertices.len() >= 2
            && self.view.scale > 0.0;
        if !clip {
            let mut polygon: Vec<PointF> = Vec::with_capacity(vertices.len() + 1);
            polygon.extend(vertices.iter().map(|&v| self.to_screen(v)));
            if closed && let Some(&first) = vertices.first() {
                polygon.push(self.to_screen(first));
            }
            self.canvas.draw_polyline(&polygon);
            return;
        }
        // The view grown by more than the pen can reach past a line's end - a square cap is half
        // the width out and half across, 0.71 of it on the diagonal - and an antialiasing pixel,
        // so a dropped segment, and the cap that replaces a join where one was dropped, lie
        // wholly outside the view and no visible pixel changes.
        let margin = (stroke_width(&pen) + 2.0) / self.view.scale;
        // Liang-Barsky decides which segments reach the view; each that does is kept WHOLE, so its
        // pixels are exactly the unclipped line's (a segment cut at the view's edge rasterises
        // with antialiasing a level or two different along its whole length). Only the segments
        // that miss the view are dropped - nearly all of a long string seen in part.
        clip_polyline_with_mode(
            vertices,
            closed,
            &self.visible.inflated(margin),
            &mut self.runs,
            PolylineClip::WholeSegments,
        );
        if self.runs.empty() {
            return;
        }
        let whole = vertices.len() + usize::from(closed);
        if self.runs.size() > 1 || self.runs.points.len() != whole {
            self.stats.lines_clipped += 1;
        }
        // Each run as a polyline of its own, through the same canvas call the whole line went
        // through: a path of several runs is stroked by another of Qt's code paths, and its
        // antialiasing differs by a level here and there along every edge.
        let view = self.view;
        for run in 0..self.runs.size() {
            let polygon: Vec<PointF> = self.runs.points[self.runs.begin(run)..self.runs.ends[run]]
                .iter()
                .map(|&point| {
                    let p = view.world_to_screen(point);
                    PointF::new(p.x, p.y)
                })
                .collect();
            self.canvas.draw_polyline(&polygon);
        }
    }

    /// `path`, in the canvas's units, as dots of the canvas's pen - square, GHOST_PEN_PIXELS a
    /// side - one every GHOST_DOT_PITCH_PIXELS from its start (cad's selection_style): even at
    /// every angle, where Qt's dotted pen drew crosses and Ts along a diagonal without
    /// antialiasing and cost several times as much with it.
    fn dot_path(&mut self, path: &[PointF], closed: bool) {
        // The walk is in device pixels where the dots go on whole ones (draw_selection_ghosts
        // sets dot_device), else in the canvas's own units; `placed` turns a dot of the walk into
        // the point drawn for it, whose square of dot_width device pixels covers whole pixels
        // only - an odd width about a pixel's centre, an even one about a pixel's corner.
        let device = self.dot_device;
        let ratio = if device { self.device_ratio } else { 1.0 };
        let shift = if device {
            self.translation
        } else {
            PointF::default()
        };
        let walked = |p: PointF| (p + shift) * ratio;
        let odd = self.dot_width % 2.0 == 1.0;
        let placed = |q: PointF| {
            if !device {
                return q;
            }
            let whole = |v: f64| if odd { v.floor() + 0.5 } else { v.round() };
            PointF::new(whole(q.x) / ratio, whole(q.y) / ratio) - shift
        };
        let reach = RectF::from_corners(
            walked(self.dot_reach.top_left()),
            walked(self.dot_reach.bottom_right()),
        );
        let count = path.len() + usize::from(closed && path.len() > 1);
        if count < 2 {
            if let Some(&first) = path.first()
                && reach.contains(walked(first))
            {
                self.canvas.draw_point(placed(walked(first)));
            }
            return;
        }
        // A dot every pitch from the path's start, the phase carried from one segment to the next
        // and through the segments that miss the view, so a pan moves no dot along the line. Only
        // the part of a segment in reach is walked (Liang-Barsky): zoomed in, a segment can be
        // millions of pixels long.
        let pitch = self.dot_pitch;
        self.dots.clear();
        let mut next = 0.0; // along the current segment, to its next dot
        for i in 0..count - 1 {
            let a = walked(path[i]);
            let b = walked(path[(i + 1) % path.len()]);
            let dx = b.x - a.x;
            let dy = b.y - a.y;
            let length = dx.hypot(dy);
            let mut from: f64 = 0.0;
            let mut to: f64 = 1.0;
            // p u <= q for each edge of the reach; false when the segment misses it.
            let mut within = |p: f64, q: f64| {
                if p == 0.0 {
                    return q >= 0.0;
                }
                let u = q / p;
                if p < 0.0 {
                    from = katana_math::cpp::max(from, u);
                } else {
                    to = katana_math::cpp::min(to, u);
                }
                from <= to
            };
            if length > 0.0
                && within(-dx, a.x - reach.left())
                && within(dx, reach.right() - a.x)
                && within(-dy, a.y - reach.top())
                && within(dy, reach.bottom() - a.y)
            {
                let first = katana_math::cpp::max(0.0, ((from * length - next) / pitch).ceil());
                let mut t = next + first * pitch;
                while t <= to * length && t < length {
                    self.dots
                        .push(placed(a + PointF::new(dx, dy) * (t / length)));
                    t += pitch;
                }
            }
            next = if next < length {
                next + ((length - next) / pitch).ceil() * pitch - length
            } else {
                next - length
            };
        }
        self.canvas.draw_points(&self.dots);
    }

    /// The selection on layers this frame hides and the document shows, as ghosts
    /// (PlanPaintOptions::selection_ghosts): over the selection's ids alone, so it costs nothing
    /// without a selection.
    fn draw_selection_ghosts(&mut self) {
        let (Some(model), Some(selection), Some(layers)) =
            (self.source.model, self.source.selection, self.frame.layers)
        else {
            return;
        };
        if !self.options.selection_ghosts || self.paper() || selection.empty() || layers.empty() {
            return;
        }
        let faint = Color::rgba(
            SELECTION_RED,
            SELECTION_GREEN,
            SELECTION_BLUE,
            GHOST_ALPHA as u8,
        );
        // Square dots, without antialiasing, each on whole device pixels: the pen's width and the
        // pitch rounded to whole device pixels, and every dot put on them (dot_path). At 125 % -
        // the owner's display (render.md) - the 2 px pen was 2.5 device pixels and landed as 2 or
        // 3: a straight ghost beaded 3, 2, 3, and a slanted one mixed four shapes of dot. Now 3 px
        // square every 8 there, and 2 every 6 at 100 % as before. Under a turned frame no pixel
        // lines up with the drawing, and the dots are laid as they fall.
        self.dot_device = merely_shifts(&self.canvas.world_transform()) && self.device_ratio > 0.0;
        self.dot_width = if self.dot_device {
            katana_math::cpp::max(1.0, (GHOST_PEN_PIXELS * self.device_ratio).round())
        } else {
            GHOST_PEN_PIXELS
        };
        self.dot_pitch = if self.dot_device {
            katana_math::cpp::max(1.0, (GHOST_DOT_PITCH_PIXELS * self.device_ratio).round())
        } else {
            GHOST_DOT_PITCH_PIXELS
        };
        let width = if self.dot_device {
            self.dot_width / self.device_ratio
        } else {
            GHOST_PEN_PIXELS
        };
        self.canvas.set_pen(Pen::with(
            faint,
            width,
            PenStyle::Solid,
            PenCap::Square,
            PenJoin::Bevel,
        ));
        self.canvas.set_brush(Brush::none());
        let antialiased = self.canvas.render_hint(RenderHint::Antialiasing);
        self.canvas.set_render_hint(RenderHint::Antialiasing, false);
        // The outline alone: no hatch, fill, linestyle or symbol, so a ghost never reads as the
        // entity drawn.
        self.hatch = None;
        self.ghost_dots = true;
        {
            let a = self.to_screen(self.visible.min);
            let b = self.to_screen(self.visible.max);
            let beyond = GHOST_PEN_PIXELS;
            self.dot_reach = RectF::from_corners(a, b)
                .normalized()
                .adjusted(-beyond, -beyond, beyond, beyond);
        }
        // What can reach the view: past it by what a paper-sized note or dimension draws beyond
        // its geometry at this scale, as the entity loop's index query reaches, and by a point's
        // ring. The cheap test, on each entity's own box, before any layout, which measures a
        // note's words: never for a note nowhere near the view.
        let reach = self.visible.inflated(katana_math::cpp::max(
            annotation_model_size(ANNOTATION_REACH_MILLIMETRES, self.options.annotation_scale),
            (GHOST_RING_PIXELS + GHOST_PEN_PIXELS) / katana_math::cpp::max(self.view.scale, 1e-12),
        ));
        let measure = self.annotation_fonts.measure();
        for id in selection.ids() {
            let Some(entity) = model.entities.find(id) else {
                continue;
            };
            // A label is placed with the others and has no outline of its own.
            if matches!(entity.geometry, Geometry::Label(_)) {
                continue;
            }
            // Drawn here already (and highlighted), or hidden by the document - which hides it in
            // every view, ghost or not.
            if !is_ghost(model, entity, layers) {
                continue;
            }
            let geometry = &entity.geometry;
            let own = bounding_box(geometry);
            if own.empty() || !own.intersects(reach) {
                continue;
            }
            // A text, a note, a leader or a dimension as the outline of the box it draws: its
            // words at ghost strength would be unreadable anyway.
            let outline: Option<Box2> = match geometry {
                Geometry::Text(text) => Some(if is_plain_text(text) {
                    bounding_box(geometry)
                } else {
                    layout_text_entity(model, text, self.options.annotation_scale, &measure).extent
                }),
                Geometry::Leader(leader) => Some(
                    build_leader(
                        model,
                        leader,
                        self.options.annotation_scale,
                        &measure,
                        self.leader_context(),
                    )
                    .extent,
                ),
                Geometry::Dimension(dimension) => {
                    let extent = build_dimension(
                        dimension,
                        resolve_dimension_style(model, entity),
                        self.options.annotation_scale,
                    )
                    .extent;
                    Some(if extent.empty() {
                        bounding_box(geometry)
                    } else {
                        extent
                    })
                }
                _ => None,
            };
            let bounds = outline.unwrap_or(own);
            let is_point = matches!(geometry, Geometry::Point(_));
            if bounds.empty() || !bounds.intersects(if is_point { reach } else { self.visible }) {
                continue;
            }
            if outline.is_some() {
                let a = self.to_screen(bounds.min);
                let b = self.to_screen(bounds.max);
                self.dot_path(&[a, PointF::new(b.x, a.y), b, PointF::new(a.x, b.y)], true);
            } else if let Geometry::Point(point) = geometry {
                // A ring of GHOST_RING_PIXELS, as a polygon fine enough that its chords are no
                // longer than the dots are apart.
                const SIDES: i32 = 24;
                let centre = self.to_screen(point.position);
                let ring: Vec<PointF> = (0..SIDES)
                    .map(|k| {
                        let angle = TWO_PI * f64::from(k) / f64::from(SIDES);
                        centre + PointF::new(angle.cos(), angle.sin()) * GHOST_RING_PIXELS
                    })
                    .collect();
                self.dot_path(&ring, true);
            } else if let Geometry::Line(segment) = geometry {
                let path = [self.to_screen(segment.start), self.to_screen(segment.end)];
                self.dot_path(&path, false);
            } else {
                self.draw_geometry(geometry);
            }
            self.stats.ghosts_drawn += 1;
        }
        self.ghost_dots = false;
        self.canvas
            .set_render_hint(RenderHint::Antialiasing, antialiased);
    }

    /// Arcs are tessellated in model space so that very large radii, where only a sliver is on
    /// screen, never hand the canvas coordinates in the millions.
    fn draw_arc_path(&mut self, arc: &Arc2) {
        let radius_pixels = arc.radius * self.view.scale;
        // Chord count for a sagitta under a quarter pixel, within sane bounds.
        let step_angle = if radius_pixels > 1.0 {
            2.0 * katana_math::cpp::max(0.0, 1.0 - 0.25 / radius_pixels).acos()
        } else {
            PI
        };
        let segments = katana_math::cpp::clamp(
            (arc.sweep.abs() / katana_math::cpp::max(step_angle, 1e-4)).ceil(),
            8.0,
            2048.0,
        ) as i32;
        let mut points = std::mem::take(&mut self.arc_points);
        points.clear();
        points.extend((0..=segments).map(|i| arc.point_at(f64::from(i) / f64::from(segments))));
        self.stroke_polyline(&points, false);
        self.arc_points = points;
    }

    fn draw_geometry(&mut self, geometry: &Geometry) {
        match geometry {
            Geometry::Point(point) => {
                let p = self.to_screen(point.position);
                let r = self.mark_pixels();
                self.canvas
                    .draw_line(p + PointF::new(-r, 0.0), p + PointF::new(r, 0.0));
                self.canvas
                    .draw_line(p + PointF::new(0.0, -r), p + PointF::new(0.0, r));
            }
            Geometry::Line(segment) => {
                let (a, b) = (self.to_screen(segment.start), self.to_screen(segment.end));
                self.canvas.draw_line(a, b);
            }
            Geometry::Arc(arc) => self.draw_arc_path(arc),
            Geometry::Circle(circle) => self.draw_arc_path(&Arc2 {
                center: circle.center,
                radius: circle.radius,
                start_angle: 0.0,
                sweep: TWO_PI,
            }),
            Geometry::Polyline(polyline) => {
                if polyline.closed && !polyline.vertices.is_empty() && self.hatch.is_some() {
                    // The fill goes down before the boundary, so the outline stays crisp over its
                    // own hatching instead of being half covered. It needs the whole boundary,
                    // visible or not.
                    let polygon: Vec<PointF> = polyline
                        .vertices
                        .iter()
                        .map(|&v| self.to_screen(v))
                        .collect();
                    self.draw_hatch(polyline, &polygon);
                }
                self.stroke_polyline(&polyline.vertices, polyline.closed);
            }
            Geometry::Text(text) => {
                if is_plain_text(text) {
                    self.draw_text(text.position, &text.text, text.height, text.rotation);
                    return;
                }
                let drawing = layout_text_entity(
                    self.annotation_model(),
                    text,
                    self.options.annotation_scale,
                    &self.annotation_fonts.measure(),
                );
                self.draw_annotation(&drawing);
            }
            // Labels are placed together by draw_labels, not one at a time.
            Geometry::Label(_) => {}
            Geometry::Leader(leader) => {
                let drawing = build_leader(
                    self.annotation_model(),
                    leader,
                    self.options.annotation_scale,
                    &self.annotation_fonts.measure(),
                    self.leader_context(),
                );
                self.draw_annotation(&drawing);
            }
            Geometry::Dimension(dimension) => {
                // Through the shared builder, so this draws exactly what the 3D view draws and
                // exactly what the cull box covers. A model-unit style is part of the drawing and
                // plots at its size on the ground; a paper-sized one is drawn for this paint's
                // scale.
                let drawing = build_dimension(
                    dimension,
                    &self.dimension_style,
                    self.options.annotation_scale,
                );
                if drawing.empty() {
                    return;
                }
                for segment in &drawing.extension_lines {
                    self.line(segment);
                }
                if drawing.has_dimension_line {
                    self.line(&drawing.dimension_line);
                }
                for curve in &drawing.curves {
                    self.stroke_polyline(curve, false);
                }
                for stroke in &drawing.arrow_strokes {
                    self.line(stroke);
                }
                for fill in &drawing.arrow_fills {
                    let polygon: Vec<PointF> = fill.iter().map(|&p| self.to_screen(p)).collect();
                    let previous = self.canvas.brush();
                    let ink = self.canvas.pen().color;
                    self.canvas.set_brush(Brush::solid(ink));
                    self.canvas.draw_polygon(&polygon, FillRule::OddEven);
                    self.canvas.set_brush(previous);
                }
                self.draw_text(
                    drawing.text_anchor,
                    &drawing.text,
                    drawing.text_height,
                    drawing.text_rotation,
                );
            }
            // The drawing system's curves, chorded in model space to a quarter of a pixel as the
            // arcs above are.
            Geometry::CurvePolyline(curve) => {
                let tolerance = 0.25 / katana_math::cpp::max(self.view.scale, 1e-12);
                if curve.closed && self.hatch.is_some() {
                    let boundary = curve.to_polyline(tolerance);
                    let polygon: Vec<PointF> = boundary
                        .vertices
                        .iter()
                        .map(|&v| self.to_screen(v))
                        .collect();
                    self.draw_hatch(&boundary, &polygon);
                }
                self.stroke_polyline(&curve.tessellate(tolerance), false);
            }
            Geometry::Ellipse(ellipse) => {
                let tolerance = 0.25 / katana_math::cpp::max(self.view.scale, 1e-12);
                self.stroke_polyline(&ellipse.tessellate(tolerance), false);
            }
            Geometry::Spline(spline) => {
                let tolerance = 0.25 / katana_math::cpp::max(self.view.scale, 1e-12);
                self.stroke_polyline(&spline.tessellate(tolerance), false);
            }
        }
    }

    fn line(&mut self, segment: &Segment2) {
        let (a, b) = (self.to_screen(segment.start), self.to_screen(segment.end));
        self.canvas.draw_line(a, b);
    }

    fn draw_alignments(&mut self) {
        let Some(model) = self.source.model else {
            return;
        };
        if model.alignments.empty() || !(self.view.scale > 0.0) {
            return;
        }
        // An overlay's sizes are the medium's: pixels on screen, millimetres of paper on a plot
        // (ALIGNMENT_*_PAPER_MILLIMETRES), so a sheet's chainages read the same at every
        // resolution.
        let line_pen = self.mark_size(2.0, ALIGNMENT_LINE_PAPER_MILLIMETRES);
        let tick_pen = self.mark_size(1.0, ALIGNMENT_TICK_PEN_PAPER_MILLIMETRES);
        // On paper the overlay's amber is a colour like any other: the plot style prints it grey
        // in greyscale and black in monochrome.
        let amber = EntityColor {
            r: ALIGNMENT.r,
            g: ALIGNMENT.g,
            b: ALIGNMENT.b,
            a: 255,
        };
        let ink = match self.options.plot {
            Some(plot) if self.paper() => canvas_colour(paper_colour(amber, plot)),
            _ => ALIGNMENT,
        };
        let tick_pixels = self.mark_size(6.0, ALIGNMENT_TICK_PAPER_MILLIMETRES);
        let text_pixels = self.mark_size(11.0, ALIGNMENT_TEXT_PAPER_MILLIMETRES);
        // About one "0+000.00" at the text size: 70 px at 11 px.
        let minimum_label_gap_pixels = 70.0 / 11.0 * text_pixels;
        // Half a pixel: finer cannot be seen, coarser shows facets on tight curves.
        let tolerance = 0.5 / self.view.scale;
        let tick = tick_pixels / self.view.scale; // screen-constant, like the snap marker
        let height = text_pixels / self.view.scale;

        for alignment in model.alignments.all() {
            // Solved per repaint. A document has a handful of alignments and the solve is a few
            // spiral end-points; caching it would need invalidation on every edit for no
            // measurable gain. Measure before changing this.
            let Ok(solved) = solve_alignment(&alignment.horizontal) else {
                continue; // the model refused it on the way in; nothing to draw
            };
            let line = solved.to_polyline(tolerance);
            if line.vertices.len() < 2 {
                continue;
            }
            let mut bounds = Box2::default();
            for &vertex in &line.vertices {
                bounds.expand(vertex);
            }
            if !bounds.inflated(tick * 4.0).intersects(self.visible) {
                continue;
            }

            let polygon: Vec<PointF> = line.vertices.iter().map(|&v| self.to_screen(v)).collect();
            self.canvas.set_pen(pen(ink, line_pen));
            self.canvas.draw_polyline(&polygon);

            self.canvas.set_pen(pen(ink, tick_pen));
            // Key stations bunch up - a 10 m spiral puts TS and SC ten metres apart - and their
            // labels then print over one another into a smear nobody can read. A label is skipped
            // when it would land within its own length of the last one drawn; the TICK is always
            // drawn, because the tick is the information and the label only names it.
            let mut last_label: Option<PointF> = None;
            for station in solved.key_stations() {
                let (Some(left), Some(right), Some(direction), Some(label)) = (
                    solved.point_at_station_offset(station, tick),
                    solved.point_at_station_offset(station, -tick),
                    solved.direction_at_station(station),
                    solved.point_at_station_offset(station, tick * 1.6),
                ) else {
                    continue;
                };
                let (a, b) = (self.to_screen(left), self.to_screen(right));
                self.canvas.draw_line(a, b);
                let at = self.to_screen(label);
                if let Some(last) = last_label
                    && (at.x - last.x).hypot(at.y - last.y) < minimum_label_gap_pixels
                {
                    continue;
                }
                last_label = Some(at);
                self.draw_text(label, &format_station(station), height, direction);
            }
            if let Some(start) = solved.point_at_station_offset(solved.start_station(), -tick * 3.0)
            {
                let direction = solved
                    .direction_at_station(solved.start_station())
                    .unwrap_or(0.0);
                self.draw_text(start, &alignment.name, height * 1.3, direction);
            }
        }
    }

    fn draw_hatch(&mut self, boundary: &Polyline2, screen: &[PointF]) {
        let Some(hatch) = self.hatch else {
            return;
        };
        let options = HatchOptions {
            view_scale: self.view.scale,
            ..HatchOptions::default()
        };
        let color = self.canvas.pen().color;

        match hatch_drawing(hatch, &options) {
            HatchDrawing::None => return,
            HatchDrawing::Solid => {
                // On screen at partial opacity rather than flat: a solid fill in the entity's own
                // colour hides the drawing underneath it, and at this zoom the user is looking at
                // the layout, not at the fill. On paper a solid fill is what it says - a plot
                // printed it at a third of its colour.
                let mut fill = color;
                if !self.paper() {
                    fill = fill.with_alpha(90);
                } else if let Some(paper_fill) = self.hatch_paper_fill {
                    fill = paper_fill;
                }
                let mut path = CanvasPath::new();
                path.add_polygon(screen);
                path.close_subpath();
                self.canvas.fill_path(&path, &Brush::solid(fill));
                return;
            }
            HatchDrawing::Lines => {}
        }

        // Hatch lines are always solid and fine, whatever the boundary is drawn with: a dashed
        // hatch of a dashed boundary is unreadable, and no CAD package draws one. On screen a
        // hairline; on paper the finest standard pen, since a PDF hairline is one device pixel
        // and its width on the page would follow the resolution.
        let previous = self.canvas.pen();
        let line_scale = self.options.plot.map_or(1.0, |plot| plot.line_weight_scale);
        let width = if self.paper() {
            HATCH_LINE_PAPER_MILLIMETRES * line_scale * self.options.pixels_per_millimetre
        } else {
            0.0
        };
        self.canvas.set_pen(pen(color, width));
        for line in hatch_segments(boundary, hatch) {
            let (a, b) = (self.to_screen(line.start), self.to_screen(line.end));
            self.canvas.draw_line(a, b);
        }
        self.canvas.set_pen(previous);
    }

    fn draw_text(&mut self, position: Point2, text: &str, height: f64, rotation: f64) {
        let pixels = height * self.view.scale;
        let anchor = self.to_screen(position);
        if pixels < 3.0 {
            // Too small to read: a stroke along the baseline keeps it discoverable.
            let width = 0.6 * pixels * text.len() as f64;
            self.canvas.draw_line(
                anchor,
                anchor + PointF::new(rotation.cos(), -rotation.sin()) * width,
            );
            return;
        }
        let size = katana_math::cpp::min(pixels, MAXIMUM_TEXT_PIXELS);
        self.canvas.save();
        self.canvas.translate(anchor.x, anchor.y);
        self.canvas.rotate(-rotation * RAD_TO_DEG); // screen y points down
        if self.paper() {
            // Exactly `size` tall: the one paper font, scaled.
            let font = self.paper_font();
            self.canvas.set_font(font);
            let scale = size / f64::from(PAPER_FONT_REFERENCE_PIXELS);
            self.canvas.scale(scale, scale);
        } else {
            let font = self.font_for(size);
            self.canvas.set_font(font);
        }
        self.canvas.draw_text(PointF::new(0.0, 0.0), text);
        self.canvas.restore();
    }

    // ---- annotation ----

    /// The model a text or leader is resolved against: the source's, or an empty one for a
    /// preview drawn with no model.
    fn annotation_model(&self) -> &'p Model {
        self.source.model.unwrap_or(empty_model())
    }

    /// What a leader's note reads beyond its target: the source's, or none.
    fn leader_context(&self) -> &'p LeaderContext {
        self.source
            .leaders
            .as_deref()
            .unwrap_or(no_leader_context())
    }

    fn annotation_target(&self, pen: Pen) -> AnnotationPaintTarget<'p> {
        let view = self.view;
        let paper = self.paper();
        let plot = self.options.plot;
        AnnotationPaintTarget {
            to_device: Box::new(move |p: Point2| {
                let s = view.world_to_screen(p);
                PointF::new(s.x, s.y)
            }),
            pixels_per_unit: view.scale,
            pen,
            colour_of: Some(Box::new(move |colour: EntityColor| {
                canvas_colour(match plot {
                    Some(plot) if paper => paper_colour(colour, plot),
                    _ => colour,
                })
            })),
            background: if paper {
                Color::WHITE
            } else {
                self.options.screen_background
            },
            minimum_text_pixels: 3.0,
        }
    }

    fn draw_annotation(&mut self, drawing: &Drawing) {
        let target = self.annotation_target(self.canvas.pen());
        paint_annotation_drawing(
            &mut *self.canvas,
            drawing,
            &target,
            &mut self.annotation_fonts,
        );
    }

    fn label_pen(&mut self, model: &'p Model, id: katana_entity::EntityId) -> Pen {
        if !self.paper() && self.source.selection.is_some_and(|s| s.contains(id)) {
            return pen(SELECTION, 2.0);
        }
        match model.entities.find(id) {
            Some(entity) => self.resolve(entity).entity_pen.clone(),
            None => self.canvas.pen(),
        }
    }

    fn draw_labels(&mut self) {
        let Some(model) = self.source.model else {
            return;
        };
        if model.label_styles.size() == 0 {
            return; // no label can have a style, so no label draws
        }
        // Every label that is drawn: its own flag and its layer, through the one visibility rule.
        // Whether it is in view is asked of its pieces, which are where its target is now, not
        // where it was made. In the model's order (its ids ascending, as for_each walks it).
        let mut labels: Vec<&'p Entity> = Vec::new();
        for id in model.entities.ids() {
            let Some(entity) = model.entities.find(id) else {
                continue;
            };
            if !matches!(entity.geometry, Geometry::Label(_)) {
                continue;
            }
            if entity.visible && self.resolve(entity).layer_drawn {
                labels.push(entity);
            }
        }
        if labels.is_empty() {
            return;
        }
        let options = LabelLayoutOptions {
            scale: self.options.annotation_scale,
            measure: self.annotation_fonts.measure(),
            visible: self.visible,
            linework: std::mem::take(&mut self.linework),
            avoid_collisions: self.options.avoid_label_collisions,
            horizontal: -self.frame.rotation,
        };
        let layout = layout_labels(model, &labels, &options);
        self.stats.labels_placed += layout.placed.len();
        self.stats.labels_displaced += layout.displaced;
        self.stats.labels_suppressed += layout.suppressed;

        for marks in layout.marks_only.iter().chain(layout.placed.iter()) {
            let label_pen = self.label_pen(model, marks.label);
            let target = self.annotation_target(label_pen);
            paint_annotation_drawing(
                &mut *self.canvas,
                &marks.drawing,
                &target,
                &mut self.annotation_fonts,
            );
        }
    }

    /// One geometry, in the canvas's current pen (paint_plan_geometry).
    fn geometry(mut self, geometry: &Geometry, dimension_style: &DimensionStyle) {
        self.hatch = None;
        self.dimension_style = dimension_style.clone();
        self.draw_geometry(geometry);
    }
}

/// The display copy of a cloud's points (geometry's point_splat): each point's colour, which the
/// pool shares out, and the coordinates the splat reads. Each colour is the pixel's RGBA bytes in
/// memory order, opaque, so a pixel the splat writes is the canvas Image's own layout.
fn display_copy(cloud: &PointCloudLayer) -> SplatCloud {
    let (minimum, maximum) = if cloud.color_mode == PointColorMode::Intensity {
        cloud
            .points
            .iter()
            .fold((f64::MAX, f64::MIN), |(low, high), point| {
                (
                    katana_math::cpp::min(low, point.intensity),
                    katana_math::cpp::max(high, point.intensity),
                )
            })
    } else {
        (cloud.bounds.min_z, cloud.bounds.max_z)
    };
    // Each point's colour is its own, so the pool can share them out: in chunks of 65 536, each
    // written by the one task that takes it.
    const GRAIN: usize = 65_536;
    let mut colours = vec![0_u32; cloud.points.len()];
    {
        let chunks: Vec<Mutex<&mut [u32]>> = colours.chunks_mut(GRAIN).map(Mutex::new).collect();
        TaskPool::shared().parallel_for(0, chunks.len(), |chunk| {
            let mut out = chunks[chunk].lock().unwrap_or_else(PoisonError::into_inner);
            let base = chunk * GRAIN;
            for (k, slot) in out.iter_mut().enumerate() {
                let rgb =
                    color_for_point(&cloud.points[base + k], cloud.color_mode, minimum, maximum);
                *slot = u32::from_ne_bytes([rgb.r, rgb.g, rgb.b, 255]);
            }
        });
    }
    // The coordinates as the splat reads them: x then y, sixteen bytes a point. The C++ reads
    // them in place through a stride; Rust copies them once per display copy rather than reading
    // the points' bytes unsafely.
    let mut coordinates = Vec::with_capacity(cloud.points.len() * 16);
    for point in &cloud.points {
        coordinates.extend_from_slice(&point.x.to_ne_bytes());
        coordinates.extend_from_slice(&point.y.to_ne_bytes());
    }
    build_splat_cloud(
        &coordinates,
        coordinates.get(8..).unwrap_or(&[]),
        16,
        cloud.points.len(),
        &colours,
        TaskPool::shared(),
    )
}

/// The splatted pixels `pixels`, starting at pixel `from` of the image, into `image`'s bytes.
fn copy_rows(pixels: &[u32], image: &mut Image, from: usize) {
    let bytes = &mut image.bytes_mut()[from * 4..(from + pixels.len()) * 4];
    for (out, &pixel) in bytes.as_chunks_mut::<4>().0.iter_mut().zip(pixels) {
        *out = pixel.to_ne_bytes();
    }
}

// ---- entry points -------------------------------------------------------------------------------

/// Paints the drawing through `frame` onto `canvas`. The canvas's state is the same afterwards as
/// before. Reentrant: see the module comment.
pub fn paint_plan(
    canvas: &mut dyn Canvas,
    source: &PlanSource<'_>,
    frame: &PlanFrame<'_>,
    options: &PlanPaintOptions<'_>,
    cache: &mut PlanPaintCache,
) -> PlanPaintStats {
    PlanPainter::new(canvas, source, frame, options, cache).paint()
}

/// One geometry, in the canvas's current pen, through `frame`: a tool's preview, which is drawn as
/// the geometry it will become. Never hatched. `source` is what the geometry reads of the drawing,
/// and only its model and leaders are read: a smart leader's note is read off the entity its tip
/// is on (and off the surfaces and alignments) every time it is drawn, so a leader previewed
/// without them is a bare arrow. None: the geometry reads nothing.
#[allow(
    clippy::too_many_arguments,
    reason = "The C++ paintPlanGeometry's arguments, the canvas first."
)]
pub fn paint_plan_geometry(
    canvas: &mut dyn Canvas,
    frame: &PlanFrame<'_>,
    options: &PlanPaintOptions<'_>,
    cache: &mut PlanPaintCache,
    geometry: &Geometry,
    dimension_style: &DimensionStyle,
    source: Option<&PlanSource<'_>>,
) {
    let nothing = PlanSource::default();
    let source = source.unwrap_or(&nothing);
    PlanPainter::new(canvas, source, frame, options, cache).geometry(geometry, dimension_style);
}

/// The model-space box the frame's viewport shows: for a rotated frame, the box that bounds the
/// rotated rectangle, so that an entity in a corner of a twisted viewport is not culled.
pub fn visible_box(frame: &PlanFrame<'_>) -> Box2 {
    let view = &frame.transform;
    if frame.rotation == 0.0 {
        return view.visible_world_bounds();
    }
    // The viewport's corners turned back by the rotation about its centre, then into the model:
    // the box of those four points is what the rotated rectangle can show. The unrotated visible
    // world bounds would leave out the corners of a twisted viewport - exactly where a sheet's
    // drawing runs into its frame.
    let mut bounds = Box2::default();
    for corner in viewport_corners(frame) {
        bounds.expand(corner);
    }
    bounds
}

/// Everything a view with these hidden layers and reference layers draws: the entities its layers
/// let through, the visible reference layers it does not hide, the shown meshes and the
/// alignments. What Zoom Extents frames and what a plot's Fit fits.
pub fn plan_drawn_bounds(
    source: &PlanSource<'_>,
    layers: &LayerOverrides,
    hidden_references: &BTreeSet<ReferenceId>,
) -> Box2 {
    let Some(model) = source.model else {
        return Box2::default();
    };
    // What THIS view draws, through the one visibility rule with this view's hidden layers: the
    // entities' own bounds counted every entity, so a layer hidden here - or a hidden stray far
    // away - still pulled the frame out to it.
    let mut bounds = model_drawn_extent(model, layers);
    // Everything the user can see, so imported imagery and point clouds count. A drawing that is
    // empty except for an orthophoto would otherwise fit an empty box and leave the photo off
    // screen. Layer by layer rather than the reference data's visible bounds, because a layer
    // hidden in this view is not seen here.
    if let Some(reference) = source.reference {
        for raster in reference.rasters() {
            if raster.visible && !hidden_references.contains(&raster.id) {
                bounds.expand_box(raster.world_bounds());
            }
        }
        for cloud in reference.point_clouds() {
            if cloud.visible && !hidden_references.contains(&cloud.id) {
                bounds.expand_box(cloud.world_bounds());
            }
        }
    }
    // The road model's outlines, as they are drawn.
    if let Some(road_model) = &source.road_model {
        bounds.expand_box(bim_view::plan_bounds(road_model, model, layers));
    }
    // A mesh is drawn in plan as its footprint, so it is seen and counts. A drawing that is
    // nothing but meshes framed an empty box before.
    for item in source.meshes.unwrap_or(&[]) {
        if let Some(mesh) = item.mesh
            && is_shown(item)
        {
            let space = mesh.bounds();
            if !space.empty() {
                bounds.expand(Point2::new(space.min.x, space.min.y));
                bounds.expand(Point2::new(space.max.x, space.max.y));
            }
        }
    }
    // Surface display (docs/surface_display.md): a visible surface is drawn, so it is seen and
    // counts - a drawing of nothing but a surface framed an empty box.
    if let Some(surfaces) = source.surfaces {
        for named in surfaces.all() {
            if let Some(surface) = named.surface.as_deref()
                && named.visible
                && !surface.empty()
            {
                bounds.expand_box(*surface.bounds());
            }
        }
    }
    // Alignments are drawn but are not entities, so they are in no entity bound. Found by looking
    // at a screenshot: the sample's access road ran off the bottom of a view that claimed to show
    // everything.
    for alignment in model.alignments.all() {
        if let Ok(solved) = solve_alignment(&alignment.horizontal) {
            // A metre is fine enough for a bounding box; the curve cannot stray further than that
            // from its chords.
            for vertex in solved.to_polyline(1.0).vertices {
                bounds.expand(vertex);
            }
        }
    }
    bounds
}

/// Whether a plan triangle has a point inside or on `area`: an edge that meets the box, or else
/// the box wholly inside the triangle, which its centre tells.
fn triangle_touches(triangle: &Triangle2, area: &Box2) -> bool {
    use katana_math::cpp::{max3, min3};
    if max3(triangle.a.x, triangle.b.x, triangle.c.x) < area.min.x
        || min3(triangle.a.x, triangle.b.x, triangle.c.x) > area.max.x
        || max3(triangle.a.y, triangle.b.y, triangle.c.y) < area.min.y
        || min3(triangle.a.y, triangle.b.y, triangle.c.y) > area.max.y
    {
        return false;
    }
    for edge in [
        Segment2::new(triangle.a, triangle.b),
        Segment2::new(triangle.b, triangle.c),
        Segment2::new(triangle.c, triangle.a),
    ] {
        if clip(&edge, area).is_some() {
            return true;
        }
    }
    triangle.contains(area.center())
}

/// Whether such a view draws anything in `area`: the drawing as a plan of it draws it (cad's
/// draws_in: the entities, hatch fills and alignments), the road model's outlines as
/// draw_road_model draws them, a visible surface by its triangles, and - by their extents, as
/// plan_drawn_bounds counts them - the imagery, point clouds and meshes. What GENERATE asks of
/// each tile in the window, so that a tile over empty ground is left out.
pub fn plan_draws_in(
    source: &PlanSource<'_>,
    layers: &LayerOverrides,
    hidden_references: &BTreeSet<ReferenceId>,
    area: &Box2,
) -> bool {
    let Some(model) = source.model else {
        return false;
    };
    if area.empty() {
        return false;
    }
    if draws_in(model, layers, area, source.index) {
        return true;
    }
    if let Some(reference) = source.reference {
        for raster in reference.rasters() {
            if raster.visible
                && !hidden_references.contains(&raster.id)
                && raster.world_bounds().intersects(*area)
            {
                return true;
            }
        }
        for cloud in reference.point_clouds() {
            if cloud.visible
                && !hidden_references.contains(&cloud.id)
                && cloud.world_bounds().intersects(*area)
            {
                return true;
            }
        }
    }
    // The road model as draw_road_model draws it: each outline along its line, and a closed one
    // filled, but for the earthworks, which are dashed and left empty.
    if let Some(road_model) = &source.road_model {
        for object in &road_model.objects {
            if object.plan.is_empty() || !bim_view::is_shown(object, model, layers) {
                continue;
            }
            for line in &object.plan {
                if touches_box(&Geometry::Polyline(line.clone()), area)
                    || (line.closed && object.kind != "earthworks" && line.contains(area.center()))
                {
                    return true;
                }
            }
        }
    }
    for item in source.meshes.unwrap_or(&[]) {
        if let Some(mesh) = item.mesh
            && is_shown(item)
        {
            let space = mesh.bounds();
            if !space.empty()
                && Box2::new(
                    Point2::new(space.min.x, space.min.y),
                    Point2::new(space.max.x, space.max.y),
                )
                .intersects(*area)
            {
                return true;
            }
        }
    }
    if let Some(surfaces) = source.surfaces {
        for named in surfaces.all() {
            let Some(surface) = named.surface.as_deref() else {
                continue;
            };
            if !named.visible || surface.empty() || !surface.bounds().intersects(*area) {
                continue;
            }
            for i in 0..surface.triangle_count() {
                if triangle_touches(&surface.plan_triangle(i), area) {
                    return true;
                }
            }
        }
    }
    false
}

/// The whole-number resolution a PDF writer is given for `dpi` (the nearest, at least 1), and the
/// canvas scale that makes a sheet laid out at exactly `dpi` land exactly on it: the writer's
/// resolution is whole, and a sheet at 300.9 dpi drawn at 300 plotted at 1 : 1003 while reporting
/// 1 : 1000 (audit QT-27).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PdfResolution {
    pub resolution: i32,
    /// resolution / dpi.
    pub scale: f64,
}

impl Default for PdfResolution {
    fn default() -> Self {
        Self {
            resolution: 300,
            scale: 1.0,
        }
    }
}

pub fn pdf_resolution_for(dpi: f64) -> PdfResolution {
    // The nearest whole resolution, so the scale that corrects it is within half a dot of 1 and
    // the writer's own units are as fine as asked for. (std::lround: half away from zero.)
    let resolution = 1.max(dpi.round() as i32);
    PdfResolution {
        resolution,
        scale: f64::from(resolution) / dpi,
    }
}

/// The frame a plot draws through: the printable area of the sheet (the paper less its margins),
/// at the sheet's scale about its centre, clipped to it, so that at a fixed scale the drawing
/// stops at the margin instead of running to the paper's edge. In the sheet's device pixels (cad's
/// Sheet). Fails with whatever cad's sheet_for refuses.
pub fn sheet_frame<'a>(settings: &PlotSettings) -> Result<PlanFrame<'a>> {
    let sheet = sheet_for(settings)?;
    // The printable area in the sheet's device pixels. The margins are equal all round, so the
    // area's centre is the paper's and the sheet's transform, sized to the area and moved in by
    // one margin, maps every model point to the same device pixel it did over the whole paper.
    let margin = settings.margin_mm * sheet.pixels_per_millimetre;
    let mut transform = sheet.view;
    transform.resize(
        sheet.width_pixels - 2.0 * margin,
        sheet.height_pixels - 2.0 * margin,
    );
    Ok(PlanFrame {
        transform,
        origin: PointF::new(margin, margin),
        clip: true,
        ..PlanFrame::default()
    })
}

/// The one page plot_plan_to_pdf asks for: QPdfWriter's settings, as plain data.
#[derive(Clone, Debug, PartialEq)]
pub struct PdfPage {
    pub path: PathBuf,
    /// Dots per inch, whole (pdf_resolution_for).
    pub resolution: i32,
    /// The document's title and creator, so a viewer's tab and a document list say what the
    /// sheet is.
    pub title: String,
    pub creator: String,
    /// The page, in millimetres; no margins (the sheet transform owns them).
    pub width_mm: f64,
    pub height_mm: f64,
}

/// Why a one-page PDF was not written: it could not be opened for writing, or not finished.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PdfPageFailure {
    Open,
    Finish,
}

/// A one-page PDF writer: QPdfWriter with its QPainter. `paint` is called once, with a canvas over
/// the page in its device pixels at `page.resolution`, unless the file cannot be opened. The
/// window's is C's PDF canvas (krilla), adapted by the integrator.
pub trait PdfPageWriter {
    fn write(
        &mut self,
        page: &PdfPage,
        paint: &mut dyn FnMut(&mut dyn Canvas),
    ) -> std::result::Result<(), PdfPageFailure>;
}

/// The one-page PDF writer over the PDF canvas (canvas::PdfCanvas, written by krilla): the page
/// in points from its millimetres, painted in device pixels at its resolution. A file that
/// cannot be created is Open, before anything is painted, as QPdfWriter's device failing to
/// open; a page or file that cannot be written is Finish. `readable` writes the file as
/// PdfDocument::uncompressed does, for a test that reads it; a plot is compressed.
#[derive(Clone, Copy, Debug, Default)]
pub struct CanvasPdfWriter {
    pub readable: bool,
}

impl PdfPageWriter for CanvasPdfWriter {
    fn write(
        &mut self,
        page: &PdfPage,
        paint: &mut dyn FnMut(&mut dyn Canvas),
    ) -> std::result::Result<(), PdfPageFailure> {
        // 72 points an inch, 25.4 millimetres an inch.
        let points = |millimetres: f64| millimetres * 72.0 / 25.4;
        if std::fs::File::create(&page.path).is_err() {
            return Err(PdfPageFailure::Open);
        }
        let mut canvas = crate::canvas::PdfCanvas::new(
            points(page.width_mm),
            points(page.height_mm),
            f64::from(page.resolution),
        );
        paint(&mut canvas);
        let mut document = if self.readable {
            crate::canvas::PdfDocument::uncompressed()
        } else {
            crate::canvas::PdfDocument::new()
        };
        document
            .add_page(canvas)
            .map_err(|_| PdfPageFailure::Finish)?;
        document
            .write(&page.path)
            .map_err(|_| PdfPageFailure::Finish)
    }
}

/// Plots `source` to a one-page PDF at `path` through sheet_frame, in paper mode, with the view's
/// hidden layers and reference layers. The PDF's title is `title` (the file's own name when
/// empty) and its creator Katana, so a viewer's tab and a document list say what the sheet is.
/// Widget-free, so it may run off the GUI thread with its own cache. Fails with FileExportFailure
/// when the file cannot be written or finished, and with whatever cad's sheet_for refuses.
/// `problems`, when given, receives the parts of a surface's style the plot is without
/// (PlanPaintStats::surface_problems: contours too fine to trace), replacing what it held; the
/// plot is written all the same.
#[allow(
    clippy::too_many_arguments,
    reason = "The C++ plotPlanToPdf's arguments, with the writer the window hands it."
)]
pub fn plot_plan_to_pdf(
    path: &Path,
    settings: &PlotSettings,
    source: &PlanSource<'_>,
    layers: &LayerOverrides,
    hidden_references: &BTreeSet<ReferenceId>,
    cache: &mut PlanPaintCache,
    title: &str,
    problems: Option<&mut Vec<SurfaceProblem>>,
    writer: &mut dyn PdfPageWriter,
) -> Status {
    let mut frame = sheet_frame(settings)?;
    frame.layers = Some(layers);
    frame.hidden_references = Some(hidden_references);
    let resolution = pdf_resolution_for(settings.dpi);
    let paper = paper_dimensions(settings.paper, settings.landscape);
    let page = PdfPage {
        path: path.to_path_buf(),
        resolution: resolution.resolution,
        // QFileInfo::completeBaseName: the name without its last suffix.
        title: if title.is_empty() {
            path.file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default()
        } else {
            String::from(title)
        },
        creator: String::from("Katana"),
        width_mm: paper.width_mm,
        height_mm: paper.height_mm,
    };
    let options = PlanPaintOptions {
        medium: PlanMedium::Paper,
        pixels_per_millimetre: millimetres_to_pixels(1.0, settings.dpi),
        plot: Some(settings),
        // Paper-sized annotation at the plot's own scale (docs/annotation.md).
        annotation_scale: settings.scale_denominator,
        ..PlanPaintOptions::default()
    };
    let mut drawn: Option<Vec<SurfaceProblem>> = None;
    let written = writer.write(&page, &mut |canvas: &mut dyn Canvas| {
        // The sheet is laid out in pixels of exactly settings.dpi; the writer's whole-number
        // resolution is brought to it by one scale.
        if resolution.scale != 1.0 {
            canvas.scale(resolution.scale, resolution.scale);
        }
        canvas.set_render_hint(RenderHint::Antialiasing, true);
        let stats = paint_plan(canvas, source, &frame, &options, cache);
        drawn = Some(stats.surface_problems);
    });
    if let (Some(problems), Some(drawn)) = (problems, drawn) {
        *problems = drawn;
    }
    let path_text = String::from_utf8_lossy(&generic_path_utf8(path)).into_owned();
    match written {
        Ok(()) => Ok(()),
        Err(PdfPageFailure::Open) => Err(make_error(
            ErrorCode::FileExportFailure,
            "could not open the PDF for writing",
            path_text,
        )),
        Err(PdfPageFailure::Finish) => Err(make_error(
            ErrorCode::FileExportFailure,
            "could not finish the PDF",
            path_text,
        )),
    }
}
