//! Turns the document into draw lists (Rule 3; include/katana/cad/scene.hpp,
//! src/katana_cad/scene.cpp).
//!
//! This is the only place that knows about both an Entity and a renderer, and
//! it is deliberately one-directional: it READS the model and WRITES
//! primitives. Nothing here can modify the document, and the renderer below
//! it cannot see the document at all, so the renderer can never become a
//! second source of truth however it is later reimplemented.
//!
//! Curves are tessellated here rather than in the renderer, because how
//! finely an arc is chorded is a modelling decision (it depends on the view
//! scale and on the tolerance the drawing is held to), not a rasterisation
//! one.
//!
//! The scene is built in LAYERS (SceneLayers), each rebuilt only when what it
//! depends on changes: a selection click rebuilds the selection overlay and
//! not the 229k-triangle surface under it. build() still makes the whole
//! scene as one list for a caller that wants that.
//!
//! The surface, mesh and fade loops have AVX2 and NEON kernels
//! (simd/scene_kernels.rs), chosen at run time and equal to the scalar code
//! here to the bit.

use katana_core::Result;
use katana_entity::annotation::DEFAULT_ANNOTATION_SCALE;
use katana_entity::display::{ResolvedDisplay, resolve_display};
use katana_entity::entity::heights_of;
use katana_entity::{Color, Entity, EntityId, Geometry, Linetype, Model};
use katana_geometry::chording::{chord_arc, chord_circle};
use katana_geometry::curves2d::arc_from_bulge;
use katana_geometry::{Point2, Point3, TriangleMesh};
use katana_math::numerics::tolerance;
use katana_math::{AABB, Vec3, cpp};
use katana_render::{
    Camera, DrawLine, DrawList, DrawTriangle, Framebuffer, Rasterizer, RenderOptions, RenderStats,
    Rgba, VertexIndex, alpha_of, blue_of, green_of, red_of, rgba, rgba_with_alpha, shade,
};
use katana_terrain::{NO_TRIANGLE, TinSurface};

use crate::dashing::{DashOptions, for_each_dash};
use crate::dimension_draw::{build_dimension, resolve_dimension_style};
use crate::document::Document;
use crate::layer_overrides::{LayerOverrides, NO_LAYER_OVERRIDES};
use crate::selection::{drawn_extent, is_drawn, is_ghost};
use crate::selection_style::{
    GHOST_COLOR3D, GHOST_WIDTH3D, SELECTED_POINT_CASING, SELECTED_POINT_SIZE,
    SELECTION_CASING_BIAS, SELECTION_CASING_COLOR, SELECTION_CASING_WIDTH, SELECTION_COLOR,
    SELECTION_CORE_BIAS, SELECTION_CORE_WIDTH,
};
use crate::simd::scene_kernels::{
    self as kernels, KernelParams, PARAM_COUNT, PARAM_GROUND, PARAM_LIFT_DATUM, PARAM_LIFT_FACTOR,
    PARAM_LIGHT_ON, PARAM_RAMP_LOW, PARAM_RAMP_SPAN, PARAM_RAMP_TABLE, PARAM_SKY_MINUS_GROUND,
    PARAM_SUN, PARAM_SUN_X, PARAM_SUN_Y, PARAM_SUN_Z, RAMP_SEGMENTS,
};
use crate::terrain::surface_colouring::{
    BandPiece, SURFACE_RAMP_STOPS, SurfaceColouring, SurfaceLegendCard, SurfaceRampStops,
    aspect_sector, aspect_sector_colour, band_pieces, surface_colouring, surface_colouring_for,
    surface_legend, surface_ramp_colour, surface_ramp_stops,
};
use crate::terrain::surface_style::{
    DENSE_SURFACE_TRIANGLES, SurfaceDisplayStyle, SurfaceFaces, SurfaceStyle,
};

// How a surface is shaded (SurfaceStyle) and DENSE_SURFACE_TRIANGLES are in
// terrain/surface_style.rs, with the display styles that choose them.

/// What decides a surface triangle's colour.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SurfaceColoring {
    /// One colour for the whole surface.
    Flat,
    /// One ramp for EVERY visible surface coloured by elevation, between the
    /// lowest and highest of them, so a height is the same colour on every
    /// surface. It was per surface, and two overlapping surfaces then gave
    /// one elevation two colours (green and orange speckle where they met).
    #[default]
    Elevation,
    /// Green to red by steepness.
    Slope,
}

#[derive(Clone, Debug)]
pub struct SceneSurface<'a> {
    pub name: String,
    pub surface: Option<&'a TinSurface>,
    pub style: SurfaceStyle,
    pub coloring: SurfaceColoring,
    pub flat_color: Rgba,
    pub visible: bool,
    /// The display style it is drawn in (terrain/surface_style.rs,
    /// docs/surface_display.md). Set, its faces3d, ramp, stops, bands, range
    /// and flat colour decide the colours, and `coloring` and `flat_color`
    /// are not read; apply_surface_display sets it with `style`, its shading.
    /// Unset, the surface is coloured as before there were styles: by
    /// `coloring`, over the one elevation ramp. A copy, not a reference into
    /// the drawing's style set, which an edit of the styles replaces.
    pub display: Option<SurfaceDisplayStyle>,
}

impl Default for SceneSurface<'_> {
    fn default() -> Self {
        Self {
            name: String::new(),
            surface: None,
            style: SurfaceStyle::Automatic,
            coloring: SurfaceColoring::Elevation,
            flat_color: rgba(170, 170, 160),
            visible: true,
            display: None,
        }
    }
}

/// `item` drawn in `style`: its 3D shading (SurfaceDisplayStyle::shading) and
/// its colours from it. Leaves the surface, its name and its visibility.
pub fn apply_surface_display(item: &mut SceneSurface<'_>, style: &SurfaceDisplayStyle) {
    item.style = style.shading;
    item.display = Some(style.clone());
    // Kept in step for a reader of the old fields; the scene reads `display`.
    item.flat_color = to_rgba(style.flat_colour);
    item.coloring = match style.faces3d {
        SurfaceFaces::Elevation => SurfaceColoring::Elevation,
        SurfaceFaces::Slope => SurfaceColoring::Slope,
        _ => SurfaceColoring::Flat,
    };
}

/// A mesh in the session: an archive's trimesh, drawn like a surface but NOT
/// one - it may be closed or overhang, so it is never sampled for a height
/// and carries no elevation ramp. Colouring is flat or per face, because that
/// is what the file gives.
#[derive(Clone, Debug)]
pub struct SceneMesh<'a> {
    pub name: String,
    pub mesh: Option<&'a TriangleMesh>,
    /// Shaded, NOT ShadedWithEdges. A mesh has no neighbour table, so every
    /// edge of every face is drawn and each line costs two triangles.
    /// Measured on an archive of 266 meshes and 126 536 faces in Release, at
    /// the framing the import leaves: with edges 886 504 triangles rasterised
    /// in 24.9 ms, without them 127 288 in 18.0 ms. On a mesh of this density
    /// the edges also read as noise. The style is per mesh, so a user who
    /// wants to see the triangulation of one asks for it.
    pub style: SurfaceStyle,
    pub flat_color: Rgba,
    /// Empty, or one per face. A face whose colour the file did not give
    /// takes flat_color, so a partly coloured mesh is still wholly drawn.
    pub face_colors: Vec<Rgba>,
    pub visible: bool,
    /// Whether the plan view draws its hull as its footprint. Off for the
    /// road model's objects, whose plan outline the plan view draws instead -
    /// a curved course's hull would cover the inside of the curve - and which
    /// the window puts in the 3D views' list with the imported meshes
    /// (docs/road_design.md, "In the views").
    pub plan_footprint: bool,
    /// The drawing layer it is shown on (a road model object's, "road/kerb");
    /// empty for an imported mesh, which no layer governs. The drawing's own
    /// hidden layers are applied where the list is made; a view's OWN hidden
    /// layers are applied by the view that draws it, which is why the layer
    /// travels with the mesh.
    pub layer: String,
}

impl Default for SceneMesh<'_> {
    fn default() -> Self {
        Self {
            name: String::new(),
            mesh: None,
            style: SurfaceStyle::Shaded,
            flat_color: rgba(190, 170, 140),
            face_colors: Vec::new(),
            visible: true,
            plan_footprint: true,
            layer: String::new(),
        }
    }
}

/// Where drawing geometry is put in z.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LineworkHeights {
    /// A vertex with a height of its own (entity's heights_of: a 3D string's
    /// `elevations`, a point's `elevation`) is drawn at it; one without is
    /// draped on the surface under it; one with no surface under it goes on
    /// the datum. The default.
    #[default]
    HeightsThenDrape,
    /// Own heights, else the datum.
    Heights,
    /// On the surface, else the datum, whatever heights it carries.
    Drape,
    /// Everything flat on the datum.
    Datum,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneOptions<'a> {
    pub draw_entities: bool,
    pub draw_grid: bool,

    pub linework: LineworkHeights,
    /// The datum is the lowest visible surface or mesh; with neither, the
    /// lowest height any entity carries; with none of those, this. Linework
    /// with no height and nothing under it goes on the datum, and so does the
    /// grid. It used to be the z of ALL linework, and the app never set it,
    /// so a drawing at 25-39 m floated through its surface at 0.
    pub entity_elevation: f64,
    /// Pixel footprints towards the eye (DrawLine::depth_bias): what lets
    /// linework draped on a surface win against it, and what is too small to
    /// show a line through a building.
    pub entity_depth_bias: f32,
    /// The selection overlay draws the selected entities again, over the
    /// drawing, and must win the tie with their own first drawing; its casing
    /// sits between the two (selection_style.rs).
    pub selection_depth_bias: f32,
    pub edge_depth_bias: f32,

    /// Vertical exaggeration. Applied to Z about `exaggeration_datum` as the
    /// scene is built, so the camera, the picking and the framing all agree
    /// about where things are. 1 is no exaggeration.
    pub vertical_exaggeration: f64,
    pub exaggeration_datum: f64,

    /// Sagitta tolerance for chording arcs and circles, in model units. The
    /// same rule the 2D viewport uses, so the two views agree.
    pub chord_tolerance: f64,

    pub default_color: Rgba,
    /// The one selection colour (selection_style.rs). It was (255, 190, 60),
    /// beside the BOUNDARY layer's (255, 213, 79) and the top of the
    /// elevation ramp, and apart from the plan view's.
    pub selection_color: Rgba,
    /// Whether the selection overlay draws a selected entity on a layer the
    /// view hides (`layers`) and the document shows, as a ghost: one faint
    /// line (selection_style.rs). Off unless the view asks - its own switch.
    pub selection_ghosts: bool,
    pub grid_color: Rgba,
    pub grid_major_color: Rgba,
    /// What the grid fades into at its rim: the view's background.
    pub grid_fade_color: Rgba,
    pub axis_color_x: Rgba,
    pub axis_color_y: Rgba,
    /// Linetypes. Off draws every line solid, which is a fast preview and a
    /// way to prove a pattern is not hiding something.
    pub draw_linetypes: bool,
    /// DXF $LTSCALE's eventual home.
    pub linetype_scale: f64,
    /// A dashed entity becomes one DrawLine per dash. Past this many spans
    /// the whole path is drawn solid instead - all or nothing, because a
    /// truncated dashed line is a shorter line with nothing to say so.
    pub maximum_dash_spans: usize,
    /// In LOGICAL pixels; pixel_scale turns them into the framebuffer's. A
    /// selected line's width is the selection's own (SELECTION_CORE_WIDTH,
    /// selection_style.rs), in the overlay and the one-list build alike.
    pub entity_line_width: f32,
    pub point_size: f32,
    /// Device pixels per logical pixel. A 3D view on a 125% display renders
    /// at its physical resolution, so a 1 px line must be 1.25 px there or it
    /// thins as the display gets sharper.
    pub pixel_scale: f32,

    /// HILLSHADE, baked per vertex on a surface and per face on a mesh. The
    /// direction TOWARDS the sun: from the north-west, 45 degrees up, the
    /// cartographic convention every hillshade uses, so relief reads as
    /// raised rather than sunken. Normalised internally; a zero vector turns
    /// the lighting off.
    pub light_direction: Vec3,
    /// Hemispheric ambient: a face pointing straight up gets sky_ambient, one
    /// pointing straight down ground_ambient, blended by the normal's z. The
    /// sun adds sun_strength * max(0, n . light) - never the absolute value,
    /// which lit faces turned away from the sun as brightly as those facing
    /// it.
    pub sky_ambient: f64,
    pub ground_ambient: f64,
    pub sun_strength: f64,

    /// Grid spacing in model units; 0 (the default) chooses a 1-2-5 spacing
    /// from the scene's size, about grid_cells_across cells over its larger
    /// side. The grid is a navigation aid, not data, so its cell count is
    /// capped however large the scene.
    pub grid_spacing: f64,
    pub grid_cells_across: i32,

    /// Layers hidden in the view being built for; None for the document rule
    /// alone. A REFERENCE to that view's state, which outlives the build, so
    /// a copy of the options made for one view never carries another's
    /// hides.
    pub layers: Option<&'a LayerOverrides>,
}

impl Default for SceneOptions<'_> {
    fn default() -> Self {
        Self {
            draw_entities: true,
            draw_grid: true,
            linework: LineworkHeights::HeightsThenDrape,
            entity_elevation: 0.0,
            entity_depth_bias: 1.5,
            selection_depth_bias: SELECTION_CORE_BIAS,
            edge_depth_bias: 1.0,
            vertical_exaggeration: 1.0,
            exaggeration_datum: 0.0,
            chord_tolerance: 0.01,
            default_color: rgba(220, 220, 220),
            selection_color: SELECTION_COLOR,
            selection_ghosts: false,
            grid_color: rgba(70, 72, 82),
            grid_major_color: rgba(92, 95, 108),
            grid_fade_color: rgba(28, 30, 36),
            axis_color_x: rgba(170, 75, 75),
            axis_color_y: rgba(75, 150, 75),
            draw_linetypes: true,
            linetype_scale: 1.0,
            maximum_dash_spans: 20000,
            entity_line_width: 1.0,
            point_size: 5.0,
            pixel_scale: 1.0,
            // The C++'s 0.70710678118654752, which is the double nearest
            // 1 / sqrt(2): FRAC_1_SQRT_2 to the bit.
            light_direction: Vec3::new(-0.5, 0.5, std::f64::consts::FRAC_1_SQRT_2),
            sky_ambient: 0.30,
            ground_ambient: 0.10,
            sun_strength: 0.80,
            grid_spacing: 0.0,
            grid_cells_across: 16,
            layers: None,
        }
    }
}

/// Per surface whose edges fade: its vertices in SceneLayers::edges, and the
/// typical length of its triangles' edges in model units (the C++
/// `SceneLayers::EdgeRun`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EdgeRun {
    pub first: VertexIndex,
    pub count: VertexIndex,
    pub typical_edge: f64,
    /// The strength edges.colors now show.
    pub applied: f32,
}

impl Default for EdgeRun {
    fn default() -> Self {
        Self {
            first: 0,
            count: 0,
            typical_edge: 0.0,
            applied: -1.0,
        }
    }
}

/// The scene split by what it depends on (the module comment). Each list is
/// drawn in the order declared, into one depth buffer.
#[derive(Clone, Debug, PartialEq)]
pub struct SceneLayers {
    /// The datum and the bounds.
    pub grid: DrawList,
    /// Surfaces and meshes.
    pub terrain: DrawList,
    /// Surface edges, faded per frame (fade_edges).
    pub edges: DrawList,
    /// The drawing.
    pub entities: DrawList,
    /// The selection's dark casing (selection_style.rs), a list of its own
    /// drawn before `selection` and writing no depth, so the core drawn after
    /// it always covers it. In the one list, the GPU - which pulls a mark
    /// nearer by its width and reads no draw-list bias - drew the wider
    /// casing over the core, and a selection was a dark line.
    pub selection_casing: DrawList,
    /// The selected entities, over the drawing.
    pub selection: DrawList,

    pub edge_runs: Vec<EdgeRun>,
    /// Parallel to edges.colors: the lit surface colour an edge fades into
    /// and the colour it has at full strength.
    pub edge_base: Vec<Rgba>,
    pub edge_ink: Vec<Rgba>,

    /// The one elevation ramp (true, unexaggerated elevations); low > high
    /// when no visible surface is coloured by elevation.
    pub ramp_low: f64,
    pub ramp_high: f64,
    /// What the view's legends show: for each drawable surface that has a
    /// display style colouring its faces by value or aspect, in drawing
    /// order, the colours as drawn, with the one ramp's range where its style
    /// gives none (surface_colouring.rs). A hidden surface, a wireframe (its
    /// mesh is one grey) and a surface with no display style have none - the
    /// last has the one ramp above, drawn as a bar when no surface has a
    /// legend.
    pub legends: Vec<SurfaceLegendCard>,
    /// Where linework with no height and nothing under it goes, exaggerated;
    /// terrain_datum when a surface or mesh decided it (build_terrain).
    pub datum: f64,
    pub terrain_datum: bool,
    /// Every vertex of terrain, edges and entities (and so of the selection's
    /// core, which draws entities again): what a view frames. Its depth range
    /// is fitted to this, the grid and the selection, whose ghosts lie
    /// outside the drawn entities. The grid is left out on purpose - it is
    /// sized from this.
    pub bounds: AABB,
    /// The part of `bounds` build_terrain found (terrain and edges), kept so a
    /// rebuild of the drawing alone need not walk the terrain again: on a
    /// 229k-triangle TIN that walk was an eighth of the build.
    pub terrain_bounds: AABB,
}

impl Default for SceneLayers {
    fn default() -> Self {
        Self {
            grid: DrawList::default(),
            terrain: DrawList::default(),
            edges: DrawList::default(),
            entities: DrawList::default(),
            selection_casing: DrawList::default(),
            selection: DrawList::default(),
            edge_runs: Vec::new(),
            edge_base: Vec::new(),
            edge_ink: Vec::new(),
            ramp_low: 0.0,
            ramp_high: -1.0,
            legends: Vec::new(),
            datum: 0.0,
            terrain_datum: false,
            bounds: AABB::default(),
            terrain_bounds: AABB::default(),
        }
    }
}

impl SceneLayers {
    pub fn has_ramp(&self) -> bool {
        self.ramp_low <= self.ramp_high
    }
}

// ---- the scalar reference --------------------------------------------------------

// The ramps are the ramp table's (surface_colouring.rs, surface_ramp_stops):
// the elevation ramp is its first, "terrain", and the kernels' parameters are
// filled from whichever ramp a surface is coloured by, so there is one copy of
// each palette. Every ramp has the kernels' four segments.
const _: () = assert!(SURFACE_RAMP_STOPS - 1 == RAMP_SEGMENTS);

// Below these the scalar loop is as quick as the kernel with its setup, so
// small surfaces, meshes and fades take it at every level. Measured with the
// thresholds at 1 (BM_SceneKernelBreakEven*, BM_SceneFadeEdges/1-4 in
// benchmarks/bench_scene.cpp; docs/performance.md): a surface of 4 to 25
// vertices was within the noise either way, a mesh of 2 faces and a fade of 10
// edge vertices already 2.5x and 3x quicker by kernel. The NEON kernels keep
// the same numbers unmeasured: each is a whole number of their steps (two
// vertices or faces, four colours), and the set-up they repay - the parameter
// block, the call - is the same whichever kernel follows.
const SURFACE_KERNEL_MINIMUM: usize = 16; // vertices
const MESH_KERNEL_MINIMUM: usize = 2; // faces
const FADE_KERNEL_MINIMUM: usize = 8; // edge vertices

/// A surface's wireframe grey.
const WIRE_COLOR: Rgba = rgba(190, 190, 190);

fn to_rgba(colour: Color) -> Rgba {
    rgba_with_alpha(colour.r, colour.g, colour.b, colour.a)
}

/// std::max on floats: the first unless the second is greater.
fn max_f32(a: f32, b: f32) -> f32 {
    if a < b { b } else { a }
}

/// Room for `extra` more, growing geometrically: a reserve of exactly what one
/// surface or mesh adds would copy the list once per surface or mesh.
fn grow<T>(list: &mut Vec<T>, extra: usize) {
    if list.capacity() - list.len() < extra {
        let wanted = std::cmp::max(list.len() + extra, 2 * list.capacity());
        list.reserve(wanted - list.len());
    }
}

/// Green (flat) through yellow to red (steep): the ramp table's "slope". 1:1
/// (45 degrees) is the red end, which is where a batter stops being
/// trafficable. For a surface with no display style; a style's Slope faces
/// take its own range and ramp (paint_of).
fn slope_ramp(slope: f64) -> Rgba {
    to_rgba(surface_ramp_colour(surface_ramp_stops("slope"), slope))
}

fn mix(a: Rgba, b: Rgba, t: f64) -> Rgba {
    let channel =
        |lo: u8, hi: u8| (f64::from(lo) + (f64::from(hi) - f64::from(lo)) * t + 0.5) as u8;
    rgba_with_alpha(
        channel(red_of(a), red_of(b)),
        channel(green_of(a), green_of(b)),
        channel(blue_of(a), blue_of(b)),
        channel(alpha_of(a), alpha_of(b)),
    )
}

/// Through the one resolution chain (entity -> style -> layer), so the 3D view
/// agrees with the 2D one. options.default_color only applies where the
/// entity has no layer at all.
fn color_of(
    model: &Model,
    entity: &Entity,
    display: &ResolvedDisplay,
    options: &SceneOptions<'_>,
) -> Rgba {
    if model.layers.find(&entity.layer).is_none() && entity.color.is_none() {
        return options.default_color;
    }
    rgba_with_alpha(
        display.color.r,
        display.color.g,
        display.color.b,
        display.color.a,
    )
}

/// The view a scene is built for: no options.layers means the document rule.
fn view_of<'a>(options: &SceneOptions<'a>) -> &'a LayerOverrides {
    options.layers.unwrap_or(&NO_LAYER_OVERRIDES)
}

/// Vertical exaggeration about its datum.
#[derive(Clone, Copy, Debug)]
struct Lift {
    factor: f64,
    datum: f64,
}

impl Lift {
    fn apply(&self, z: f64) -> f64 {
        self.datum + (z - self.datum) * self.factor
    }
}

fn lift_of(options: &SceneOptions<'_>) -> Lift {
    Lift {
        factor: if options.vertical_exaggeration.is_finite() && options.vertical_exaggeration > 0.0
        {
            options.vertical_exaggeration
        } else {
            1.0
        },
        datum: if options.exaggeration_datum.is_finite() {
            options.exaggeration_datum
        } else {
            0.0
        },
    }
}

/// The hillshade of SceneOptions, for a UNIT normal.
#[derive(Clone, Copy, Debug)]
struct Light {
    towards_sun: Vec3,
    on: bool,
    sky: f64,
    ground: f64,
    sun: f64,
}

impl Light {
    fn intensity(&self, normal: Vec3) -> f64 {
        if !self.on {
            return 1.0;
        }
        let hemisphere = self.ground + (self.sky - self.ground) * (0.5 + 0.5 * normal.z);
        cpp::clamp(
            hemisphere + self.sun * cpp::max(0.0, normal.dot(self.towards_sun)),
            0.0,
            1.0,
        )
    }
}

fn light_of(options: &SceneOptions<'_>) -> Light {
    let length = options.light_direction.length();
    let on = length > 1.0e-9 && length.is_finite();
    Light {
        towards_sun: if on {
            options.light_direction / length
        } else {
            Vec3::new(0.0, 0.0, 1.0)
        },
        on,
        sky: cpp::clamp(options.sky_ambient, 0.0, 1.0),
        ground: cpp::clamp(options.ground_ambient, 0.0, 1.0),
        sun: cpp::clamp(options.sun_strength, 0.0, 1.0),
    }
}

/// The surface of an item that can be drawn: visible, present, with
/// triangles, and not Hidden.
fn drawable_surface<'a>(item: &SceneSurface<'a>) -> Option<&'a TinSurface> {
    let surface = item.surface?;
    (item.visible && !surface.empty() && item.style != SurfaceStyle::Hidden).then_some(surface)
}

fn drawable_mesh<'a>(item: &SceneMesh<'a>) -> Option<&'a TriangleMesh> {
    let mesh = item.mesh?;
    (item.visible && !mesh.empty() && item.style != SurfaceStyle::Hidden).then_some(mesh)
}

fn style_of(item: &SceneSurface<'_>, surface: &TinSurface) -> SurfaceStyle {
    if item.style != SurfaceStyle::Automatic {
        return item.style;
    }
    if surface.triangle_count() > DENSE_SURFACE_TRIANGLES {
        SurfaceStyle::Shaded
    } else {
        SurfaceStyle::ShadedWithEdges
    }
}

#[derive(Clone, Copy, Debug)]
struct Ramp {
    low: f64,
    high: f64,
    /// The table's stops; the elevation ramp's unless a style names another.
    stops: &'static SurfaceRampStops,
}

impl Default for Ramp {
    fn default() -> Self {
        Self {
            low: 0.0,
            high: -1.0,
            stops: surface_ramp_stops("terrain"),
        }
    }
}

impl Ramp {
    fn at(&self, z: f64) -> Rgba {
        let span = self.high - self.low;
        to_rgba(surface_ramp_colour(
            self.stops,
            if span > tolerance::GEOMETRIC {
                (z - self.low) / span
            } else {
                0.5
            },
        ))
    }

    fn spans(&self) -> bool {
        self.low <= self.high
    }
}

/// Whether a surface's heights stretch the one elevation ramp: coloured by
/// elevation, by a ramp (stops are absolute), with an end of its range left
/// to the scene. A wireframe colours nothing - its lines are one grey - so its
/// heights stretch no ramp and, alone, bring up no legend bar.
fn shares_ramp(item: &SceneSurface<'_>, surface: &TinSurface) -> bool {
    if style_of(item, surface) == SurfaceStyle::Wireframe {
        return false;
    }
    let Some(style) = &item.display else {
        return item.coloring == SurfaceColoring::Elevation;
    };
    style.faces3d == SurfaceFaces::Elevation
        && style.stops.is_empty()
        && !(style.range_min.is_some() && style.range_max.is_some())
}

/// Lowest and highest of every visible surface that shares the elevation
/// ramp.
fn ramp_of(surfaces: &[SceneSurface<'_>]) -> Ramp {
    let mut ramp = Ramp {
        low: f64::INFINITY,
        high: f64::NEG_INFINITY,
        ..Ramp::default()
    };
    for item in surfaces {
        if let Some(surface) = drawable_surface(item)
            && shares_ramp(item, surface)
        {
            ramp.low = cpp::min(ramp.low, surface.min_elevation());
            ramp.high = cpp::max(ramp.high, surface.max_elevation());
        }
    }
    if !(ramp.low <= ramp.high) {
        ramp = Ramp::default();
    }
    ramp
}

/// The shared vertices' colours: one flat colour, the ramp at each vertex's
/// height (what the kernels can do), or the colouring's colour of each height
/// (stops, bands: the scalar loop).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PaintVertex {
    Flat,
    Ramp,
    Value,
}

/// The triangles: on the shared vertices; on copies of their own, a colour a
/// face (slope, aspect); or cut into their bands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PaintFaces {
    Shared,
    PerFace,
    Bands,
}

/// How one surface's colours are worked out: by the rule from before display
/// styles (no style), or by its style (docs/surface_display.md, "In 3D").
#[derive(Clone, Debug)]
struct SurfacePaint {
    vertex: PaintVertex,
    faces: PaintFaces,
    ramp: Ramp,
    flat: Rgba,
    /// A display style's rule, resolved; absent for a surface with none.
    colouring: Option<SurfaceColouring>,
}

fn paint_of(item: &SceneSurface<'_>, surface: &TinSurface, shared: &Ramp) -> SurfacePaint {
    let mut paint = SurfacePaint {
        vertex: PaintVertex::Flat,
        faces: PaintFaces::Shared,
        ramp: Ramp::default(),
        flat: item.flat_color,
        colouring: None,
    };
    let Some(style) = &item.display else {
        match item.coloring {
            SurfaceColoring::Flat => {}
            SurfaceColoring::Elevation => {
                paint.vertex = PaintVertex::Ramp;
                paint.ramp = *shared;
            }
            SurfaceColoring::Slope => paint.faces = PaintFaces::PerFace,
        }
        return paint;
    };
    paint.flat = to_rgba(style.flat_colour);
    match style.faces3d {
        SurfaceFaces::None | SurfaceFaces::Flat => {}
        SurfaceFaces::Elevation => {
            // The scene's one range where the style leaves an end open, so a
            // height is one colour on every surface in the view; the
            // surface's own when nothing shares it.
            let shared_range = shared.spans();
            let colouring = surface_colouring(
                style,
                SurfaceFaces::Elevation,
                if shared_range {
                    shared.low
                } else {
                    surface.min_elevation()
                },
                if shared_range {
                    shared.high
                } else {
                    surface.max_elevation()
                },
            );
            if style.banded {
                paint.vertex = PaintVertex::Value;
                paint.faces = PaintFaces::Bands;
            } else if !style.stops.is_empty() || colouring.collapsed() {
                // A range too narrow to stretch the ramp over is a point, and
                // a height below or above it is the ramp's start or end: the
                // colouring's rule, which the kernels' ramp (the middle only)
                // does not know.
                paint.vertex = PaintVertex::Value;
            } else {
                paint.vertex = PaintVertex::Ramp;
                paint.ramp.low = colouring.low;
                paint.ramp.high = colouring.high;
                paint.ramp.stops = surface_ramp_stops(&style.ramp);
            }
            paint.colouring = Some(colouring);
        }
        SurfaceFaces::Slope => {
            paint.colouring = Some(surface_colouring_for(style, SurfaceFaces::Slope, surface));
            paint.faces = PaintFaces::PerFace;
        }
        SurfaceFaces::Aspect => {
            paint.colouring = Some(surface_colouring_for(style, SurfaceFaces::Aspect, surface));
            paint.faces = PaintFaces::PerFace;
        }
    }
    paint
}

/// A face's own colour (PaintFaces::PerFace), before the light.
fn face_colour(paint: &SurfacePaint, surface: &TinSurface, triangle: usize) -> Rgba {
    let aspect = surface.triangle_slope_aspect(triangle);
    let Some(colouring) = &paint.colouring else {
        return slope_ramp(aspect.map_or(0.0, |aspect| aspect.slope));
    };
    let Some(aspect) = aspect else {
        return to_rgba(colouring.flat); // vertical in plan: no slope to colour
    };
    if colouring.faces == SurfaceFaces::Aspect {
        return match aspect.aspect {
            Some(direction) => to_rgba(aspect_sector_colour(aspect_sector(direction))),
            None => to_rgba(colouring.flat),
        };
    }
    to_rgba(colouring.colour_at(aspect.slope * 100.0))
}

/// Lowest visible surface or mesh, unexaggerated; None when there is none.
fn terrain_floor(surfaces: &[SceneSurface<'_>], meshes: &[SceneMesh<'_>]) -> Option<f64> {
    let mut lowest: Option<f64> = None;
    let mut take = |z: f64| {
        if z.is_finite() {
            lowest = Some(match lowest {
                Some(low) => cpp::min(low, z),
                None => z,
            });
        }
    };
    for item in surfaces {
        if let Some(surface) = drawable_surface(item) {
            take(surface.min_elevation());
        }
    }
    for item in meshes {
        if let Some(mesh) = drawable_mesh(item) {
            let space = mesh.bounds();
            if !space.empty() {
                take(space.min.z);
            }
        }
    }
    lowest
}

// ---- draping ---------------------------------------------------------------------

/// The visible surfaces linework can be draped on, with their plan boxes so a
/// point outside one costs a comparison rather than a locate.
struct Drape<'a> {
    surfaces: Vec<&'a TinSurface>,
}

impl<'a> Drape<'a> {
    fn new(surfaces: &[SceneSurface<'a>]) -> Self {
        Self {
            surfaces: surfaces.iter().filter_map(drawable_surface).collect(),
        }
    }

    fn empty(&self) -> bool {
        self.surfaces.is_empty()
    }

    /// The TOP surface at p - the one seen from above - and its elevation.
    fn at(&self, p: Point2) -> Option<(f64, usize)> {
        let mut best: Option<(f64, usize)> = None;
        for (k, surface) in self.surfaces.iter().enumerate() {
            if !surface.bounds().contains(p) {
                continue;
            }
            if let Some(z) = surface.elevation_at(p)
                && best.is_none_or(|(top, _)| z > top)
            {
                best = Some((z, k));
            }
        }
        best
    }

    /// Appends, in order, where the segment a-b crosses an edge of surface k,
    /// with the surface's elevation there: the vertices that make a straight
    /// chord lie ON a TIN rather than cutting through its ridges and valleys.
    /// Walks triangle to triangle through the neighbour table, so it costs one
    /// step per edge crossed; it stops where the surface does.
    fn crossings(&self, k: usize, a: Point2, b: Point2, out: &mut Vec<Point3>) {
        let surface = self.surfaces[k];
        let Some(start) = surface.locate(a) else {
            return;
        };
        let vertices = surface.vertices();
        let dx = b.x - a.x;
        let dy = b.y - a.y;
        let mut triangle = start.triangle;
        let mut reached = 0.0;
        // A bound, not an expectation: one step per triangle crossed.
        for _step in 0..surface.triangle_count() {
            let corners = surface.triangles()[triangle as usize];
            let mut exit = f64::INFINITY;
            let mut exit_edge: Option<usize> = None;
            for e in 0..3 {
                let p = vertices[corners[e] as usize];
                let q = vertices[corners[(e + 1) % 3] as usize];
                let ex = q.x - p.x;
                let ey = q.y - p.y;
                // The direction against the edge's outward normal (ey, -ex)
                // of a counter-clockwise triangle: > 0 is on the way out.
                let outward = dx * ey - dy * ex;
                if !(outward > 0.0) {
                    continue;
                }
                let s = ((p.x - a.x) * ey - (p.y - a.y) * ex) / outward;
                if s < exit {
                    exit = s;
                    exit_edge = Some(e);
                }
            }
            // b is in this triangle, or the walk is not getting anywhere (a
            // degenerate triangle): the chord from here is on the surface.
            let Some(exit_edge) = exit_edge else {
                return;
            };
            if !(exit < 1.0) || exit < reached - 1.0e-9 {
                return;
            }
            let p = vertices[corners[exit_edge] as usize];
            let q = vertices[corners[(exit_edge + 1) % 3] as usize];
            let cx = a.x + dx * exit;
            let cy = a.y + dy * exit;
            if exit > reached + 1.0e-12 {
                // On the edge, so the height is the edge's: the same from the
                // triangles either side of it.
                let ex = q.x - p.x;
                let ey = q.y - p.y;
                let length2 = ex * ex + ey * ey;
                let u = if length2 > 0.0 {
                    cpp::clamp(((cx - p.x) * ex + (cy - p.y) * ey) / length2, 0.0, 1.0)
                } else {
                    0.0
                };
                out.push(Point3::new(cx, cy, p.z + (q.z - p.z) * u));
                reached = exit;
            }
            let next = surface.neighbors()[triangle as usize][exit_edge];
            if next == NO_TRIANGLE {
                return; // off the edge of the surface
            }
            triangle = next;
        }
    }
}

/// Puts drawing vertices in z by the options' rule (LineworkHeights). A vertex
/// whose z is left NaN goes on the datum, which is known only once every
/// entity has been seen when there is no terrain; the caller fills those in.
struct Heights<'a> {
    drape: Drape<'a>,
    mode: LineworkHeights,
    lift: Lift,
    scratch: Vec<Point3>,
}

impl<'a> Heights<'a> {
    fn new(surfaces: &[SceneSurface<'a>], options: &SceneOptions<'_>) -> Self {
        Self {
            drape: Drape::new(surfaces),
            mode: options.linework,
            lift: lift_of(options),
            scratch: Vec::new(),
        }
    }

    fn uses_own_heights(&self) -> bool {
        self.mode == LineworkHeights::Heights || self.mode == LineworkHeights::HeightsThenDrape
    }

    fn drapes(&self) -> bool {
        !self.drape.empty()
            && (self.mode == LineworkHeights::Drape
                || self.mode == LineworkHeights::HeightsThenDrape)
    }

    /// A path of plan points with their own heights (None where a vertex has
    /// none) as 3D points, draped and densified where the rule says so.
    fn path(
        &mut self,
        points: &[Point2],
        own: &[Option<f64>],
        closed: bool,
        out: &mut Vec<Point3>,
        lowest: &mut f64,
    ) {
        out.clear();
        let count = points.len();
        let uses_own = self.uses_own_heights();
        let drapes = self.drapes();
        let mut previous_surface: Option<usize> = None;
        for i in 0..=count {
            if i == count && !(closed && count > 2) {
                break;
            }
            let index = i % count;
            let p = points[index];
            let height = if uses_own && index < own.len() {
                own[index]
            } else {
                None
            };
            let mut surface: Option<usize> = None;
            let mut z = f64::NAN;
            match height {
                Some(height) if height.is_finite() => {
                    z = self.lift.apply(height);
                    *lowest = cpp::min(*lowest, height);
                }
                _ => {
                    if drapes && let Some((top, k)) = self.drape.at(p) {
                        z = self.lift.apply(top);
                        surface = Some(k);
                    }
                }
            }
            // Densify a chord whose two ends are both draped on one surface.
            if i > 0
                && let (Some(here), Some(before)) = (surface, previous_surface)
                && here == before
            {
                self.scratch.clear();
                self.drape
                    .crossings(here, points[(i - 1) % count], p, &mut self.scratch);
                for crossing in &self.scratch {
                    out.push(Point3::new(
                        crossing.x,
                        crossing.y,
                        self.lift.apply(crossing.z),
                    ));
                }
            }
            out.push(Point3::new(p.x, p.y, z));
            previous_surface = surface;
        }
    }
}

/// A 1-2-5 step at least `wanted`: 1, 2, 5, 10, 20, 50...
fn nice_step(wanted: f64) -> f64 {
    if !(wanted > 0.0) || !wanted.is_finite() {
        return 10.0;
    }
    let decade = 10.0_f64.powf(wanted.log10().floor());
    for m in [1.0, 2.0, 5.0, 10.0] {
        if m * decade >= wanted * (1.0 - 1.0e-12) {
            return m * decade;
        }
    }
    10.0 * decade
}

/// The elevation ramp's colour at t in [0, 1] (clamped): what the legend of a
/// view draws, so it cannot disagree with the surfaces. The ramp table's
/// first ramp, "terrain" (surface_ramp_colour), to the bit.
pub fn elevation_ramp_color(t: f64) -> Rgba {
    // Linear ramp through a small fixed palette - the ramp table's first,
    // "terrain". Chosen over a single hue so that adjacent contour bands are
    // actually distinguishable, which is the whole point of colouring by
    // elevation.
    to_rgba(surface_ramp_colour(surface_ramp_stops("terrain"), t))
}

// ---- surfaces --------------------------------------------------------------------

/// What the kernels are handed: the scalar code's constants, laid out as the
/// PARAM_* indices of simd/scene_kernels.rs say.
fn kernel_params(lift: &Lift, light: &Light, ramp: &Ramp) -> KernelParams {
    let mut params = [0.0; PARAM_COUNT];
    params[PARAM_LIFT_FACTOR] = lift.factor;
    params[PARAM_LIFT_DATUM] = lift.datum;
    params[PARAM_RAMP_LOW] = ramp.low;
    params[PARAM_RAMP_SPAN] = ramp.high - ramp.low;
    params[PARAM_LIGHT_ON] = if light.on { 1.0 } else { 0.0 };
    params[PARAM_SUN_X] = light.towards_sun.x;
    params[PARAM_SUN_Y] = light.towards_sun.y;
    params[PARAM_SUN_Z] = light.towards_sun.z;
    params[PARAM_GROUND] = light.ground;
    params[PARAM_SKY_MINUS_GROUND] = light.sky - light.ground;
    params[PARAM_SUN] = light.sun;
    let palette = ramp.stops;
    let channel_of = |channel: usize, i: usize| {
        let c = palette[i];
        f64::from(match channel {
            0 => c.r,
            1 => c.g,
            _ => c.b,
        })
    };
    for channel in 0..3 {
        for i in 0..RAMP_SEGMENTS {
            let stop = channel_of(channel, i);
            params[PARAM_RAMP_TABLE + 2 * channel * RAMP_SEGMENTS + i] = stop;
            params[PARAM_RAMP_TABLE + (2 * channel + 1) * RAMP_SEGMENTS + i] =
                channel_of(channel, i + 1) - stop;
        }
    }
    params
}

/// The scratch emit_surface reuses across surfaces and builds.
struct SurfaceScratch<'s> {
    normals: &'s mut Vec<Vec3>,
    lifted: &'s mut Vec<f64>,
    summed: &'s mut Vec<f64>,
}

/// Where a surface's fading edges go: SceneLayers' edges and their runs (the
/// C++'s `SceneLayers* edges`, the parts of the layers it touches).
struct EdgeSink<'e> {
    edges: &'e mut DrawList,
    base: &'e mut Vec<Rgba>,
    ink: &'e mut Vec<Rgba>,
    runs: &'e mut Vec<EdgeRun>,
}

/// The list a surface's edges are drawn into: the sink's edges when they fade
/// and there is a sink, else `out`.
fn edge_target<'t>(
    out: &'t mut DrawList,
    sink: &'t mut Option<EdgeSink<'_>>,
    to_sink: bool,
) -> &'t mut DrawList {
    match sink {
        Some(sink) if to_sink => &mut *sink.edges,
        _ => out,
    }
}

/// One surface into `out`, coloured as `paint` says (paint_of), and its fading
/// edges into `sink` (or into `out` at full strength when there is none).
/// Shared by append_surfaces and build_terrain.
fn emit_surface(
    item: &SceneSurface<'_>,
    surface: &TinSurface,
    options: &SceneOptions<'_>,
    paint: &SurfacePaint,
    scratch: SurfaceScratch<'_>,
    out: &mut DrawList,
    mut sink: Option<EdgeSink<'_>>,
) {
    let lift = lift_of(options);
    let light = light_of(options);
    let style = style_of(item, surface);
    let shaded = style == SurfaceStyle::Shaded || style == SurfaceStyle::ShadedWithEdges;
    let wire = style == SurfaceStyle::Wireframe;
    let fading = style == SurfaceStyle::ShadedWithEdges;
    let ramp = &paint.ramp;
    let ramp_vertices = paint.vertex == PaintVertex::Ramp;
    let shared_faces = paint.faces == PaintFaces::Shared;
    let vertices = surface.vertices();
    let triangles = surface.triangles();
    let scale = max_f32(options.pixel_scale, 0.1);

    // Per-vertex normals for smooth hillshade: the area-weighted sum of the
    // faces round the vertex, each turned to face UP. A TIN is a height field,
    // so up is its outside; the old abs() did the same for the light but let
    // a face turned AWAY from the sun look as lit as one facing it.
    let smooth = shaded && light.on && shared_faces;
    let base = out.positions.len() as VertexIndex;
    let count = vertices.len();
    // Sized once and written in place: resize grows the arrays geometrically
    // where a reserve per surface would copy them once per surface.
    out.positions
        .resize(base as usize + count, Point3::default());
    out.colors.resize(base as usize + count, 0);
    {
        let positions = &mut out.positions[base as usize..];
        let colors = &mut out.colors[base as usize..];
        // The kernels colour a vertex flat or by a ramp; a colour a value
        // (stops, bands) is the scalar loop's.
        if count >= SURFACE_KERNEL_MINIMUM
            && paint.vertex != PaintVertex::Value
            && let Some(set) = kernels::kernels()
        {
            let params = kernel_params(&lift, &light, ramp);
            scratch.lifted.resize(4 * count, 0.0);
            kernels::lift(set, vertices, &params, &mut scratch.lifted[..4 * count]);
            if smooth {
                scratch.summed.clear();
                scratch.summed.resize(4 * count, 0.0);
                kernels::surface_normals(
                    set,
                    &scratch.lifted[..4 * count],
                    triangles,
                    &mut scratch.summed[..4 * count],
                );
            }
            // The ramp's degenerate case (one elevation) is one colour: flat.
            let ramped = !wire && ramp_vertices && ramp.high - ramp.low > tolerance::GEOMETRIC;
            let flat = if wire {
                WIRE_COLOR
            } else if ramp_vertices {
                ramp.at(ramp.low)
            } else {
                paint.flat
            };
            kernels::surface_vertices(
                set,
                &scratch.lifted[..4 * count],
                if smooth {
                    Some(&scratch.summed[..4 * count])
                } else {
                    None
                },
                &params,
                flat,
                ramped,
                positions,
                colors,
            );
        } else {
            // The scalar reference, which the kernels equal to the bit.
            let normals = &mut *scratch.normals;
            if smooth {
                normals.clear();
                normals.resize(count, Vec3::new(0.0, 0.0, 0.0));
                for t in triangles {
                    let a = vertices[t[0] as usize];
                    let b = vertices[t[1] as usize];
                    let c = vertices[t[2] as usize];
                    let pa = Vec3::new(a.x, a.y, lift.apply(a.z));
                    let mut n = (Vec3::new(b.x, b.y, lift.apply(b.z)) - pa)
                        .cross(Vec3::new(c.x, c.y, lift.apply(c.z)) - pa);
                    if n.z < 0.0 {
                        n *= -1.0;
                    }
                    for &corner in t {
                        normals[corner as usize] += n;
                    }
                }
            }
            // One vertex per TIN vertex, shared by every triangle that uses
            // it: one matrix multiply per vertex rather than three per
            // triangle. Lighting per vertex is what makes the sharing
            // possible; it used to be per facet, on private copies, six times
            // the vertices.
            for (i, vertex) in vertices.iter().enumerate() {
                let mut color = paint.flat;
                if wire {
                    color = WIRE_COLOR;
                } else if ramp_vertices {
                    color = ramp.at(vertex.z);
                } else if paint.vertex == PaintVertex::Value
                    && let Some(colouring) = &paint.colouring
                {
                    color = to_rgba(colouring.colour_at(vertex.z));
                }
                if smooth {
                    let length = normals[i].length();
                    if length > 0.0 {
                        color = shade(color, light.intensity(normals[i] / length));
                    }
                }
                positions[i] = Vec3::new(vertex.x, vertex.y, lift.apply(vertex.z));
                colors[i] = color;
            }
        }
    }

    if shaded && shared_faces {
        out.triangles
            .extend(triangles.iter().map(|corners| DrawTriangle {
                a: base + corners[0],
                b: base + corners[1],
                c: base + corners[2],
            }));
    } else if shaded && paint.faces == PaintFaces::Bands {
        // Each triangle cut where a band edge - a contour - crosses it
        // (band_pieces, on the TRUE heights the bands are of), each piece in
        // its band's colour, lit by the triangle's own normal: a band stops at
        // its contour rather than smearing across the triangle.
        if let Some(colouring) = &paint.colouring {
            grow(&mut out.triangles, 2 * triangles.len());
            grow(&mut out.positions, 4 * triangles.len());
            grow(&mut out.colors, 4 * triangles.len());
            let mut pieces: Vec<BandPiece> = Vec::new();
            for corners in triangles {
                let a = out.positions[(base + corners[0]) as usize];
                let b = out.positions[(base + corners[1]) as usize];
                let c = out.positions[(base + corners[2]) as usize];
                let mut n = (b - a).cross(c - a);
                if n.z < 0.0 {
                    n *= -1.0;
                }
                let length = n.length();
                let lit = length > 1.0e-12;
                let intensity = if lit {
                    light.intensity(n / length)
                } else {
                    1.0
                };
                pieces.clear();
                band_pieces(
                    colouring,
                    vertices[corners[0] as usize],
                    vertices[corners[1] as usize],
                    vertices[corners[2] as usize],
                    &mut pieces,
                );
                for piece in &pieces {
                    let mut color = to_rgba(colouring.band_colour(piece.band));
                    if lit {
                        color = shade(color, intensity);
                    }
                    let first = out.positions.len() as VertexIndex;
                    for corner in &piece.corners[..piece.count] {
                        out.add_vertex(Vec3::new(corner.x, corner.y, lift.apply(corner.z)), color);
                    }
                    // A fan: each piece is convex and keeps the triangle's
                    // turn.
                    for k in 1..piece.count.saturating_sub(1) {
                        out.add_triangle(
                            first,
                            first + k as VertexIndex,
                            first + (k + 1) as VertexIndex,
                        );
                    }
                }
            }
        }
    } else if shaded {
        grow(&mut out.triangles, triangles.len());
        grow(&mut out.positions, 3 * triangles.len());
        grow(&mut out.colors, 3 * triangles.len());
        for (t, corners) in triangles.iter().enumerate() {
            let a = out.positions[(base + corners[0]) as usize];
            let b = out.positions[(base + corners[1]) as usize];
            let c = out.positions[(base + corners[2]) as usize];
            // Slope and aspect are properties of the FACET: private copies,
            // or sharing would smear them across the surface. Lit by the
            // facet normal.
            let mut color = face_colour(paint, surface, t);
            let mut n = (b - a).cross(c - a);
            if n.z < 0.0 {
                n *= -1.0;
            }
            let length = n.length();
            if length > 1.0e-12 {
                color = shade(color, light.intensity(n / length));
            }
            let a2 = out.add_vertex(a, color);
            let b2 = out.add_vertex(b, color);
            let c2 = out.add_vertex(c, color);
            out.add_triangle(a2, b2, c2);
        }
    }

    if !wire && !fading {
        return;
    }
    // Faded edges go to their own list; the rest into `out`.
    let to_sink = fading && sink.is_some();
    let run_start = edge_target(out, &mut sink, to_sink).positions.len() as VertexIndex;
    let neighbours = surface.neighbors();
    let drawn_here =
        |t: usize, e: usize| neighbours[t][e] == NO_TRIANGLE || neighbours[t][e] >= t as u32;
    // Reserved once for the lot: these lists hold every surface's edges, and
    // growing them edge by edge cost more than drawing them.
    let mut drawn = 0;
    for t in 0..triangles.len() {
        for e in 0..3 {
            drawn += usize::from(drawn_here(t, e));
        }
    }
    {
        let target = edge_target(out, &mut sink, to_sink);
        grow(&mut target.lines, drawn);
        grow(&mut target.positions, 2 * drawn);
        grow(&mut target.colors, 2 * drawn);
    }
    if to_sink && let Some(sink) = &mut sink {
        grow(&mut *sink.base, 2 * drawn);
        grow(&mut *sink.ink, 2 * drawn);
    }
    // An ordinary edge's ink at each vertex: the same shade() of the same
    // colour for every edge meeting there, so worked out once.
    let ink: Vec<Rgba> = if wire {
        Vec::new()
    } else {
        // Not `colors` above: the lists have grown since.
        out.colors[base as usize..base as usize + count]
            .iter()
            .map(|&colour| shade(colour, 0.55))
            .collect()
    };
    let strong = rgba(250, 210, 120);
    for (t, corners) in triangles.iter().enumerate() {
        for e in 0..3 {
            // Each interior edge is shared by two triangles; drawing it from
            // the lower-indexed one only halves the line count.
            if !drawn_here(t, e) {
                continue;
            }
            let a = base + corners[e];
            let b = base + corners[(e + 1) % 3];
            // A breakline or boundary edge is drawn heavier: on a survey
            // surface those are the edges that mean something.
            let constrained = surface.is_edge_constrained(t, e);
            if wire && !constrained {
                // The surface's own grey vertices.
                edge_target(out, &mut sink, to_sink).lines.push(DrawLine {
                    a,
                    b,
                    width: scale,
                    depth_bias: 0.0,
                });
                continue;
            }
            let width = (if constrained { 2.0 } else { 1.0 }) * scale;
            let bias = if wire { 0.0 } else { options.edge_depth_bias };
            // An ordinary edge is a darker shade of the surface under it, so
            // it fades INTO the surface rather than into black.
            let ink_a = if constrained {
                strong
            } else {
                ink[(a - base) as usize]
            };
            let ink_b = if constrained {
                strong
            } else {
                ink[(b - base) as usize]
            };
            let position_a = out.positions[a as usize];
            let position_b = out.positions[b as usize];
            let colour_a = out.colors[a as usize];
            let colour_b = out.colors[b as usize];
            let target = edge_target(out, &mut sink, to_sink);
            let a2 = target.positions.len() as VertexIndex;
            target.positions.push(position_a);
            target.positions.push(position_b);
            target.colors.push(ink_a);
            target.colors.push(ink_b);
            target.lines.push(DrawLine {
                a: a2,
                b: a2 + 1,
                width,
                depth_bias: bias,
            });
            if to_sink && let Some(sink) = &mut sink {
                sink.base.push(colour_a);
                sink.base.push(colour_b);
                sink.ink.push(ink_a);
                sink.ink.push(ink_b);
            }
        }
    }
    if to_sink && let Some(sink) = &mut sink {
        // The side of an equilateral triangle of the mean plan area: what a
        // typical edge is, whatever the TIN's regularity.
        let mean_area = surface.plan_area() / surface.triangle_count() as f64;
        sink.runs.push(EdgeRun {
            first: run_start,
            count: sink.edges.positions.len() as VertexIndex - run_start,
            typical_edge: (cpp::max(mean_area, 0.0) * 4.0 / 1.7320508075688772).sqrt(),
            // Built at full strength (the ink).
            applied: 1.0,
        });
    }
}

// ---- entities --------------------------------------------------------------------

/// Which entities an emit draws, and how: every one as drawn; the selected
/// ones drawn in the view, as the overlay's core (selection_style.rs); every
/// one, the selected in the core's colour and width (the one-list build); the
/// selected ones the view alone hides, as ghosts. Only and Ghosts walk ids
/// rather than the drawing: `walk` when given, else the selection's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Selected {
    AsDrawn,
    Only,
    Styled,
    Ghosts,
}

/// What every drawn path goes through (emit_entities): the list, the heights,
/// the draped path's scratch and the lowest own height seen.
struct PathEmitter<'o, 'a, 'b> {
    out: &'o mut DrawList,
    heights: Heights<'a>,
    options: &'b SceneOptions<'b>,
    /// The vertices' own heights, set for each entity before its paths.
    own: Vec<Option<f64>>,
    path: Vec<Point3>,
    /// The plan of the draped path, which the dashes are laid along.
    plan: Vec<Point2>,
    lowest: f64,
    base_bias: f32,
    point_size: f32,
    scale: f32,
}

/// The height along the draped `path` at the plan point `q`, the dashes being
/// laid along its plan in order, so one cursor serves the whole path.
fn z_at(path: &[Point3], cursor: &mut usize, q: Point2) -> f64 {
    while *cursor + 1 < path.len() {
        let p0 = path[*cursor];
        let p1 = path[*cursor + 1];
        let ex = p1.x - p0.x;
        let ey = p1.y - p0.y;
        let length2 = ex * ex + ey * ey;
        let t = if length2 > 0.0 {
            ((q.x - p0.x) * ex + (q.y - p0.y) * ey) / length2
        } else {
            1.0
        };
        if t <= 1.0 + 1.0e-9 || *cursor + 2 >= path.len() {
            return p0.z + (p1.z - p0.z) * cpp::clamp(t, 0.0, 1.0);
        }
        *cursor += 1;
    }
    path[path.len() - 1].z
}

impl PathEmitter<'_, '_, '_> {
    /// EVERY drawn path goes through this one emitter, including a plain
    /// Segment2. The tempting shortcut - a Segment2 adding its DrawLine
    /// directly - would have left lines solid in 3D while polylines, arcs and
    /// circles dashed: the most confusing possible half-feature.
    fn emit_path(
        &mut self,
        points: &[Point2],
        closed: bool,
        color: Rgba,
        width: f32,
        linetype: Option<&Linetype>,
    ) {
        if points.is_empty() {
            return;
        }
        self.heights
            .path(points, &self.own, closed, &mut self.path, &mut self.lowest);
        if points.len() == 1 {
            let v = self.out.add_vertex(self.path[0], color);
            self.out
                .add_point(v, self.point_size * self.scale, self.base_bias);
            return;
        }

        if self.options.draw_linetypes
            && let Some(linetype) = linetype
            && !linetype.is_continuous()
        {
            // A 3D view has no single pixels-per-model-unit, so resolvability
            // cannot be judged here; the span budget bounds the work instead.
            let dash = DashOptions {
                pattern_scale: self.options.linetype_scale,
                view_scale: 1.0,
                minimum_element_pixels: 0.0,
                maximum_spans: self.options.maximum_dash_spans,
            };
            // The dashes are laid along the plan of the DRAPED path, so each
            // end takes its height from the 3D segment it falls on.
            self.plan.clear();
            self.plan
                .extend(self.path.iter().map(|p| Point2::new(p.x, p.y)));
            let mut cursor = 0;
            let path = &self.path;
            let out = &mut *self.out;
            let dot_size = self.options.point_size * 0.6 * self.scale;
            let base_bias = self.base_bias;
            // The draped path already holds the closing vertex when closed.
            let laid = for_each_dash(&self.plan, false, linetype, &dash, |a, b| {
                let from = out.add_vertex(Vec3::new(a.x, a.y, z_at(path, &mut cursor, a)), color);
                if a.distance_to(b) <= 0.0 {
                    out.add_point(from, dot_size, base_bias);
                    return;
                }
                let to = out.add_vertex(Vec3::new(b.x, b.y, z_at(path, &mut cursor, b)), color);
                out.add_line(from, to, width, base_bias);
            });
            if laid {
                return;
            }
        }

        let mut previous = self.out.add_vertex(self.path[0], color);
        for &position in &self.path[1..] {
            let current = self.out.add_vertex(position, color);
            self.out.add_line(previous, current, width, self.base_bias);
            previous = current;
        }
    }
}

/// Builds draw lists. Held across frames so its buffers are reused rather
/// than reallocated on every redraw.
#[derive(Clone, Debug, Default)]
pub struct SceneBuilder {
    /// Tessellation scratch, reused.
    chord: Vec<Point2>,
    /// Per-vertex normal scratch, reused.
    normals: Vec<Vec3>,
    /// The kernels' scratch (simd/scene_kernels.rs): four doubles a vertex of
    /// lifted positions and of summed normals.
    lifted: Vec<f64>,
    summed: Vec<f64>,
    /// Bounds scratch: which vertices a primitive uses.
    used: Vec<u8>,
}

impl SceneBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends the whole scene to `out`, which is cleared first: grid,
    /// terrain, edges at full strength, and the drawing with the selection in
    /// the selection style. One list cannot carry the depth rules
    /// render_layers draws the grid and the edges with, so a surface flat at
    /// the datum shows the grid through it here; a view draws SceneLayers.
    pub fn build(
        &mut self,
        document: &Document,
        surfaces: &[SceneSurface<'_>],
        options: &SceneOptions<'_>,
        out: &mut DrawList,
        meshes: &[SceneMesh<'_>],
    ) {
        out.clear();
        let mut layers = SceneLayers::default();
        self.build_terrain(surfaces, meshes, options, &mut layers);
        let mut entities = DrawList::default();
        let mut lowest = f64::INFINITY;
        if options.draw_entities {
            self.emit_entities(
                document,
                surfaces,
                options,
                Selected::Styled,
                layers.datum,
                layers.terrain_datum,
                &mut entities,
                Some(&mut lowest),
                None,
            );
            if !layers.terrain_datum && lowest.is_finite() {
                layers.datum = lift_of(options).apply(lowest);
            }
            layers
                .bounds
                .expand(entities.bounds_with_scratch(&mut self.used));
        }
        let append = |out: &mut DrawList, from: &DrawList| {
            let base = out.positions.len() as VertexIndex;
            out.positions.extend_from_slice(&from.positions);
            out.colors.extend_from_slice(&from.colors);
            for t in &from.triangles {
                out.add_triangle(base + t.a, base + t.b, base + t.c);
            }
            for l in &from.lines {
                out.add_line(base + l.a, base + l.b, l.width, l.depth_bias);
            }
            for p in &from.points {
                out.add_point(base + p.a, p.size, p.depth_bias);
            }
        };
        if options.draw_grid {
            let mut grid = DrawList::default();
            emit_grid(options, &layers.bounds, layers.datum, &mut grid);
            append(out, &grid);
        }
        append(out, &layers.terrain);
        append(out, &layers.edges);
        append(out, &entities);
    }

    // ---- by layer --------------------------------------------------------------

    /// Terrain and edges, the ramp and the part of the datum surfaces and
    /// meshes decide. Everything else depends on it, so it comes first.
    pub fn build_terrain(
        &mut self,
        surfaces: &[SceneSurface<'_>],
        meshes: &[SceneMesh<'_>],
        options: &SceneOptions<'_>,
        layers: &mut SceneLayers,
    ) {
        layers.terrain.clear();
        layers.edges.clear();
        layers.edge_runs.clear();
        layers.edge_base.clear();
        layers.edge_ink.clear();
        let ramp = ramp_of(surfaces);
        layers.ramp_low = ramp.low;
        layers.ramp_high = ramp.high;
        // The legends: each drawable surface whose display style colours its
        // faces by a value or by aspect, in drawing order, with the one
        // ramp's range where its style leaves an end open - its colours as
        // they are drawn.
        layers.legends.clear();
        // Where each surface's edges begin in edges.lines.
        let mut edge_starts = Vec::new();
        for item in surfaces {
            let Some(surface) = drawable_surface(item) else {
                continue;
            };
            // Worked out once a surface: a slope style's steepest face is a
            // pass over every triangle.
            let paint = paint_of(item, surface, &ramp);
            if let Some(display) = &item.display
                && style_of(item, surface) != SurfaceStyle::Wireframe
                && let Some(colouring) = &paint.colouring
                && colouring.colours_faces()
            {
                layers.legends.push(SurfaceLegendCard {
                    surface: item.name.clone(),
                    style: display.name.clone(),
                    legend: surface_legend(colouring),
                });
            }
            edge_starts.push(layers.edges.lines.len());
            emit_surface(
                item,
                surface,
                options,
                &paint,
                SurfaceScratch {
                    normals: &mut self.normals,
                    lifted: &mut self.lifted,
                    summed: &mut self.summed,
                },
                &mut layers.terrain,
                Some(EdgeSink {
                    edges: &mut layers.edges,
                    base: &mut layers.edge_base,
                    ink: &mut layers.edge_ink,
                    runs: &mut layers.edge_runs,
                }),
            );
        }
        // The edges are drawn writing no depth (render_layers), so where two
        // surfaces coincide - a design repeating the existing triangles
        // outside the works - the one drawn later covers the other. The
        // terrain goes the other way: at a tie the surface drawn FIRST is the
        // one seen. So the edges go surface by surface in reverse, and the
        // edges on top are those of the surface seen, as they were while
        // edges wrote depth.
        if edge_starts.len() > 1 {
            let mut reversed = Vec::with_capacity(layers.edges.lines.len());
            let mut end = layers.edges.lines.len();
            for &start in edge_starts.iter().rev() {
                reversed.extend_from_slice(&layers.edges.lines[start..end]);
                end = start;
            }
            layers.edges.lines = reversed;
        }
        self.append_meshes(meshes, options, &mut layers.terrain);
        let floor = terrain_floor(surfaces, meshes);
        layers.terrain_datum = floor.is_some();
        layers.datum = lift_of(options).apply(match floor {
            Some(floor) => floor,
            None => {
                if options.entity_elevation.is_finite() {
                    options.entity_elevation
                } else {
                    0.0
                }
            }
        });
        layers.terrain_bounds = layers.terrain.bounds_with_scratch(&mut self.used);
        layers
            .terrain_bounds
            .expand(layers.edges.bounds_with_scratch(&mut self.used));
        layers.bounds = layers.terrain_bounds;
    }

    /// The drawing, draped on the visible surfaces. May lower the datum (to
    /// the lowest entity height) when there is no terrain.
    pub fn build_entities(
        &mut self,
        document: &Document,
        surfaces: &[SceneSurface<'_>],
        options: &SceneOptions<'_>,
        layers: &mut SceneLayers,
    ) {
        layers.entities.clear();
        if !options.draw_entities {
            return;
        }
        let mut lowest = f64::INFINITY;
        self.emit_entities(
            document,
            surfaces,
            options,
            Selected::AsDrawn,
            layers.datum,
            layers.terrain_datum,
            &mut layers.entities,
            Some(&mut lowest),
            None,
        );
        if !layers.terrain_datum {
            layers.datum = lift_of(options).apply(if lowest.is_finite() {
                lowest
            } else if options.entity_elevation.is_finite() {
                options.entity_elevation
            } else {
                0.0
            });
        }
        layers.bounds = layers.terrain_bounds;
        layers
            .bounds
            .expand(layers.entities.bounds_with_scratch(&mut self.used));
    }

    pub fn build_selection(
        &mut self,
        document: &Document,
        surfaces: &[SceneSurface<'_>],
        options: &SceneOptions<'_>,
        layers: &mut SceneLayers,
    ) {
        layers.selection.clear();
        layers.selection_casing.clear();
        if !options.draw_entities || document.selection().empty() {
            return;
        }
        self.emit_entities(
            document,
            surfaces,
            options,
            Selected::Only,
            layers.datum,
            true,
            &mut layers.selection,
            None,
            None,
        );
        // The casing: every line and point of the core again, wider, darker
        // and a little behind it (selection_style.rs), in the list drawn
        // before the core's and writing no depth (SceneLayers::
        // selection_casing).
        let scale = max_f32(options.pixel_scale, 0.1);
        let overlay = &layers.selection;
        let casing = &mut layers.selection_casing;
        for core in &overlay.lines {
            let a = casing.add_vertex(overlay.positions[core.a as usize], SELECTION_CASING_COLOR);
            let b = casing.add_vertex(overlay.positions[core.b as usize], SELECTION_CASING_COLOR);
            casing.add_line(a, b, SELECTION_CASING_WIDTH * scale, SELECTION_CASING_BIAS);
        }
        for core in &overlay.points {
            let vertex =
                casing.add_vertex(overlay.positions[core.a as usize], SELECTION_CASING_COLOR);
            casing.add_point(vertex, SELECTED_POINT_CASING * scale, SELECTION_CASING_BIAS);
        }
        // The selection on layers this view alone hides, faint and without a
        // casing, when the view asks for it.
        if options.selection_ghosts && options.layers.is_some_and(|view| !view.empty()) {
            self.emit_entities(
                document,
                surfaces,
                options,
                Selected::Ghosts,
                layers.datum,
                true,
                &mut layers.selection,
                None,
                None,
            );
        }
    }

    /// The box the entities `ids` take in the scene built into `layers` with
    /// `options`: where build_selection would draw them were they the
    /// selection - the ones the view draws, and with its ghosts on the ones
    /// it ghosts - draped and on the datum as the drawing is. What a 3D view
    /// frames for ZOOM on a scope (view_verbs). Empty when the view draws none
    /// of them, or draws no entities at all.
    pub fn overlay_bounds(
        &mut self,
        document: &Document,
        surfaces: &[SceneSurface<'_>],
        options: &SceneOptions<'_>,
        layers: &SceneLayers,
        ids: &[EntityId],
    ) -> AABB {
        if !options.draw_entities || ids.is_empty() {
            return AABB::default();
        }
        // Through the overlay's own emits, on the datum the drawing was put
        // at, so the box is where the view draws them - draped, lifted,
        // exaggerated - and never a second rule for where an entity stands in
        // 3D.
        let mut scratch = DrawList::default();
        self.emit_entities(
            document,
            surfaces,
            options,
            Selected::Only,
            layers.datum,
            true,
            &mut scratch,
            None,
            Some(ids),
        );
        if options.selection_ghosts && options.layers.is_some_and(|view| !view.empty()) {
            self.emit_entities(
                document,
                surfaces,
                options,
                Selected::Ghosts,
                layers.datum,
                true,
                &mut scratch,
                None,
                Some(ids),
            );
        }
        scratch.bounds_with_scratch(&mut self.used)
    }

    /// From layers.bounds and layers.datum, so after the others.
    pub fn build_grid(&mut self, options: &SceneOptions<'_>, layers: &mut SceneLayers) {
        layers.grid.clear();
        if options.draw_grid {
            emit_grid(options, &layers.bounds, layers.datum, &mut layers.grid);
        }
    }

    /// Recolours the fading edges for how big the triangles now are on screen
    /// through `camera`: gone below about 4 px, full above about 12. Touches
    /// edges.colors only when a surface's strength changes by a step. False
    /// when no edge has any strength, and the edges list need not be drawn.
    pub fn fade_edges(layers: &mut SceneLayers, camera: &Camera) -> bool {
        let mut any = false;
        // The size of a pixel where the camera looks: one number per frame,
        // so the whole surface fades together rather than in bands by
        // distance.
        let pixel = camera.world_per_pixel_at(camera.distance());
        for run in &mut layers.edge_runs {
            let pixels = if pixel > 0.0 {
                run.typical_edge / pixel
            } else {
                0.0
            };
            // Nothing below 4 px - the triangles are then texture, not
            // structure - and full strength from 12; in eighths, so orbiting
            // does not recolour a million vertices a frame for an invisible
            // change.
            let t = cpp::clamp((pixels - 4.0) / 8.0, 0.0, 1.0);
            let strength = ((t * t * (3.0 - 2.0 * t) * 8.0).round() / 8.0) as f32;
            if strength != run.applied {
                let first = run.first as usize;
                let end = std::cmp::min(
                    run.first.wrapping_add(run.count) as usize,
                    layers.edges.colors.len(),
                );
                if end >= first + FADE_KERNEL_MINIMUM
                    && end <= layers.edge_base.len()
                    && end <= layers.edge_ink.len()
                    && let Some(set) = kernels::kernels()
                {
                    // strength is a whole number of eighths (above), exactly.
                    kernels::fade(
                        set,
                        &layers.edge_base[first..end],
                        &layers.edge_ink[first..end],
                        (strength * 8.0) as i32,
                        &mut layers.edges.colors[first..end],
                    );
                    run.applied = strength;
                    any = any || strength > 0.0;
                    continue;
                }
                if first < end {
                    for ((colour, &base), &ink) in layers.edges.colors[first..end]
                        .iter_mut()
                        .zip(&layers.edge_base[first..end])
                        .zip(&layers.edge_ink[first..end])
                    {
                        *colour = mix(base, ink, f64::from(strength));
                    }
                }
                run.applied = strength;
            }
            any = any || strength > 0.0;
        }
        any
    }

    // ---- single pieces, for a view that shows one thing ------------------------

    pub fn append_surfaces(
        &mut self,
        surfaces: &[SceneSurface<'_>],
        options: &SceneOptions<'_>,
        out: &mut DrawList,
    ) {
        let ramp = ramp_of(surfaces);
        for item in surfaces {
            if let Some(surface) = drawable_surface(item) {
                emit_surface(
                    item,
                    surface,
                    options,
                    &paint_of(item, surface, &ramp),
                    SurfaceScratch {
                        normals: &mut self.normals,
                        lifted: &mut self.lifted,
                        summed: &mut self.summed,
                    },
                    out,
                    None,
                );
            }
        }
    }

    /// A mesh is drawn like a surface, with two differences that follow from
    /// what it is: no elevation ramp (a closed mesh has no "the height
    /// here"), and its edges are drawn from every face rather than once per
    /// shared edge, because a TriangleMesh carries no neighbour table.
    ///
    /// Lit per FACE by the face's own winding, and never abs(): a wall turned
    /// away from the sun is darker than one facing it, and a face turned down
    /// is lit only by the ground's ambient. That is what makes a mesh's shape
    /// readable.
    pub fn append_meshes(
        &mut self,
        meshes: &[SceneMesh<'_>],
        options: &SceneOptions<'_>,
        out: &mut DrawList,
    ) {
        let lift = lift_of(options);
        let light = light_of(options);
        let scale = max_f32(options.pixel_scale, 0.1);

        // Room for every mesh at once: a trimesh archive is hundreds of small
        // meshes, and growing the lists face by face was most of their cost.
        let mut vertex_count = 0;
        let mut triangle_count = 0;
        for item in meshes {
            if let Some(mesh) = drawable_mesh(item) {
                // Shaded and edge copies at most.
                vertex_count += 6 * mesh.triangle_count();
                triangle_count += mesh.triangle_count();
            }
        }
        grow(&mut out.positions, vertex_count);
        grow(&mut out.colors, vertex_count);
        grow(&mut out.triangles, triangle_count);
        let kernel_set = kernels::kernels();
        let params = kernel_params(&lift, &light, &Ramp::default());

        for item in meshes {
            let Some(mesh) = drawable_mesh(item) else {
                continue;
            };
            let shaded = item.style == SurfaceStyle::Shaded
                || item.style == SurfaceStyle::ShadedWithEdges
                || item.style == SurfaceStyle::Automatic;
            let edges = item.style == SurfaceStyle::Wireframe
                || item.style == SurfaceStyle::ShadedWithEdges;
            let edge_color = if shaded {
                rgba(40, 40, 45)
            } else {
                rgba(190, 190, 190)
            };
            let bias = if shaded { options.edge_depth_bias } else { 0.0 };

            // Shaded without edges - how a mesh is drawn unless asked
            // otherwise - is three private vertices and a triangle a face:
            // the kernel's shape. It takes each run of faces that name real
            // vertices; a face that does not is skipped, as triangle()
            // refuses it below.
            if let Some(set) = kernel_set
                && shaded
                && !edges
                && mesh.triangle_count() >= MESH_KERNEL_MINIMUM
            {
                let faces = mesh.triangle_count();
                let points = mesh.vertices.len();
                let valid = |f: usize| {
                    mesh.faces[f]
                        .iter()
                        .all(|&corner| (corner as usize) < points)
                };
                let mut f = 0;
                while f < faces {
                    if !valid(f) {
                        f += 1;
                        continue;
                    }
                    let mut end = f + 1;
                    while end < faces && valid(end) {
                        end += 1;
                    }
                    let run = end - f;
                    let first = out.positions.len();
                    out.positions.resize(first + 3 * run, Point3::default());
                    out.colors.resize(first + 3 * run, 0);
                    kernels::mesh_faces(
                        set,
                        &mesh.vertices,
                        &mesh.faces,
                        f,
                        run,
                        &params,
                        &item.face_colors,
                        item.flat_color,
                        &mut out.positions[first..],
                        &mut out.colors[first..],
                    );
                    for k in 0..run {
                        let a = (first + 3 * k) as VertexIndex;
                        // Corners c, b, a.
                        out.add_triangle(a + 2, a + 1, a);
                    }
                    f = end;
                }
                continue;
            }
            for f in 0..mesh.triangle_count() {
                // triangle() refuses a face that names a vertex which does
                // not exist, so an unvalidated mesh cannot be read past its
                // end here.
                let Some(face) = mesh.triangle(f) else {
                    continue;
                };
                let base = if f < item.face_colors.len() {
                    item.face_colors[f]
                } else {
                    item.flat_color
                };
                let a = Vec3::new(face.a.x, face.a.y, lift.apply(face.a.z));
                let b = Vec3::new(face.b.x, face.b.y, lift.apply(face.b.z));
                let c = Vec3::new(face.c.x, face.c.y, lift.apply(face.c.z));
                if shaded {
                    let mut color = base;
                    let normal = (b - a).cross(c - a);
                    let area = normal.length();
                    if area > 1.0e-12 {
                        color = shade(base, light.intensity(normal / area));
                    }
                    // The last corner first: the order these lists have
                    // always had, from when the three add_vertex calls were
                    // add_triangle's arguments and GCC evaluated them right
                    // to left. Spelt out so it no longer rests on the
                    // compiler; the kernel matches.
                    let vc = out.add_vertex(c, color);
                    let vb = out.add_vertex(b, color);
                    let va = out.add_vertex(a, color);
                    out.add_triangle(va, vb, vc);
                }
                if edges {
                    let ea = out.add_vertex(a, edge_color);
                    let eb = out.add_vertex(b, edge_color);
                    let ec = out.add_vertex(c, edge_color);
                    out.add_line(ea, eb, scale, bias);
                    out.add_line(eb, ec, scale, bias);
                    out.add_line(ec, ea, scale, bias);
                }
            }
        }
    }

    /// Flat on the datum rule without surfaces to drape on.
    pub fn append_entities(
        &mut self,
        document: &Document,
        options: &SceneOptions<'_>,
        out: &mut DrawList,
    ) {
        self.emit_entities(
            document,
            &[],
            options,
            Selected::Styled,
            0.0,
            false,
            out,
            None,
            None,
        );
    }

    /// A grid round `around` at options.entity_elevation.
    pub fn append_grid(&mut self, options: &SceneOptions<'_>, around: &AABB, out: &mut DrawList) {
        if !options.draw_grid {
            return;
        }
        let z = lift_of(options).apply(if options.entity_elevation.is_finite() {
            options.entity_elevation
        } else {
            0.0
        });
        emit_grid(options, around, z, out);
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "The C++ member's arguments, the walk's ids among them."
    )]
    fn emit_entities(
        &mut self,
        document: &Document,
        surfaces: &[SceneSurface<'_>],
        options: &SceneOptions<'_>,
        which: Selected,
        datum_hint: f64,
        datum_known: bool,
        out: &mut DrawList,
        lowest_height: Option<&mut f64>,
        walk: Option<&[EntityId]>,
    ) {
        let model = document.model();
        let selection = document.selection();
        let scale = max_f32(options.pixel_scale, 0.1);
        let first_vertex = out.positions.len();

        // The overlay - its core and its ghosts - is drawn over the drawing
        // and walks the selection's ids alone: O(selection), not O(drawing).
        let overlay = which == Selected::Only || which == Selected::Ghosts;
        let mut emitter = PathEmitter {
            out: &mut *out,
            heights: Heights::new(surfaces, options),
            options,
            own: Vec::new(),
            path: Vec::new(),
            plan: Vec::new(),
            lowest: f64::INFINITY,
            base_bias: if overlay {
                options.selection_depth_bias
            } else {
                options.entity_depth_bias
            },
            point_size: if which == Selected::Only {
                SELECTED_POINT_SIZE
            } else {
                options.point_size
            },
            scale,
        };
        let mut chord = std::mem::take(&mut self.chord);
        let view = view_of(options);

        let mut emit_one = |entity: &Entity| {
            // The plan view's rules, so a layer switched off disappears here
            // too: drawn where this view draws it, a ghost where only this
            // view hides it (selection.rs).
            let shown = if which == Selected::Ghosts {
                is_ghost(model, entity, view)
            } else {
                is_drawn(model, entity, view)
            };
            if !shown {
                return;
            }
            // The overlay walks the selection, so everything it meets is
            // selected.
            let selected = overlay || selection.contains(entity.id);
            let styled = selected && which != Selected::AsDrawn;
            let display = resolve_display(model, entity);
            let color = if which == Selected::Ghosts {
                GHOST_COLOR3D
            } else if styled {
                options.selection_color
            } else {
                color_of(model, entity, &display, options)
            };
            let width = (if which == Selected::Ghosts {
                GHOST_WIDTH3D
            } else if styled {
                SELECTION_CORE_WIDTH
            } else {
                options.entity_line_width
            }) * scale;
            // A selected entity is drawn in the selection style, dashes and
            // all.
            let linetype = if styled {
                None
            } else {
                model.linetypes.find(&display.linetype)
            };
            let uses_own = emitter.heights.uses_own_heights();
            let own_heights = |own: &mut Vec<Option<f64>>, count: usize| {
                own.clear();
                if uses_own {
                    *own = heights_of(&entity.properties, count);
                }
            };

            match &entity.geometry {
                Geometry::Point(shape) => {
                    own_heights(&mut emitter.own, 1);
                    chord.clear();
                    chord.push(shape.position);
                    emitter.emit_path(&chord, false, color, width, None);
                }
                Geometry::Line(shape) => {
                    own_heights(&mut emitter.own, 2);
                    chord.clear();
                    chord.extend([shape.start, shape.end]);
                    emitter.emit_path(&chord, false, color, width, linetype);
                }
                Geometry::Arc(shape) => {
                    // An arc's heights are its two ends (set at import): not
                    // per chord vertex, so the chords drape instead.
                    emitter.own.clear();
                    chord = chord_arc(shape, options.chord_tolerance);
                    emitter.emit_path(&chord, false, color, width, linetype);
                }
                Geometry::Circle(shape) => {
                    emitter.own.clear();
                    chord = chord_circle(shape, options.chord_tolerance);
                    if uses_own {
                        // One height, the centre's, for the whole circle.
                        let centre = heights_of(&entity.properties, 1);
                        emitter.own = vec![centre[0]; chord.len()];
                    }
                    emitter.emit_path(&chord, true, color, width, linetype);
                }
                Geometry::Polyline(shape) => {
                    own_heights(&mut emitter.own, shape.vertices.len());
                    emitter.emit_path(&shape.vertices, shape.closed, color, width, linetype);
                }
                Geometry::Text(shape) => {
                    // Text is not rendered in 3D yet: a glyph outline needs a
                    // font, and drawing a marker where the text sits is more
                    // honest than drawing nothing (the 2D view shows the
                    // text).
                    own_heights(&mut emitter.own, 1);
                    chord.clear();
                    chord.push(shape.position);
                    emitter.emit_path(&chord, false, color, width, None);
                }
                Geometry::Dimension(shape) => {
                    // Drawn through the same builder the 2D viewport uses, so
                    // the two views cannot disagree about what a dimension
                    // looks like. The label is not rendered here - a glyph
                    // outline needs a font, which the 3D path does not have.
                    let drawn = build_dimension(
                        shape,
                        resolve_dimension_style(model, entity),
                        DEFAULT_ANNOTATION_SCALE,
                    );
                    if drawn.empty() {
                        return;
                    }
                    emitter.own.clear();
                    let mut stroke = |from: Point2, to: Point2| {
                        chord.clear();
                        chord.extend([from, to]);
                        emitter.emit_path(&chord, false, color, width, None);
                    };
                    for segment in &drawn.extension_lines {
                        stroke(segment.start, segment.end);
                    }
                    if drawn.has_dimension_line {
                        stroke(drawn.dimension_line.start, drawn.dimension_line.end);
                    }
                    for curve in &drawn.curves {
                        for pair in curve.windows(2) {
                            stroke(pair[0], pair[1]);
                        }
                    }
                    for segment in &drawn.arrow_strokes {
                        stroke(segment.start, segment.end);
                    }
                    for fill in &drawn.arrow_fills {
                        // Outlined rather than filled: DrawList fills only
                        // triangles, and an arrowhead is small enough that an
                        // outline reads correctly.
                        for i in 0..fill.len() {
                            stroke(fill[i], fill[(i + 1) % fill.len()]);
                        }
                    }
                }
                Geometry::Label(shape) => {
                    // As a text is: a marker where the label attaches. Its
                    // words are worked out at a scale the 3D view does not
                    // have, and set in a font it does not either.
                    emitter.own.clear();
                    chord.clear();
                    chord.push(shape.anchor);
                    emitter.emit_path(&chord, false, color, width, None);
                }
                Geometry::Leader(shape) => {
                    // The line; the arrow and the note are paper-sized.
                    emitter.own.clear();
                    emitter.emit_path(&shape.vertices, false, color, width, None);
                }
                Geometry::CurvePolyline(shape) => {
                    // The heights are the geometry's own, one per vertex; each
                    // chord point of an arc takes the height interpolated by
                    // length along its segment, so a 3D string with arcs
                    // rises smoothly round them.
                    emitter.own.clear();
                    chord.clear();
                    for i in 0..shape.segment_count() {
                        let from = shape.vertices[i];
                        let to = shape.vertices[shape.segment_end(i)];
                        let piece = match arc_from_bulge(from.position, to.position, from.bulge) {
                            Some(arc) => chord_arc(&arc, options.chord_tolerance),
                            None => vec![from.position, to.position],
                        };
                        let start = if chord.is_empty() { 0 } else { 1 };
                        for (k, &point) in piece.iter().enumerate().skip(start) {
                            chord.push(point);
                            if uses_own {
                                let t = k as f64 / (piece.len() - 1) as f64;
                                emitter.own.push(match (from.height, to.height) {
                                    (Some(low), Some(high)) => Some(low + (high - low) * t),
                                    _ => None,
                                });
                            }
                        }
                    }
                    if shape.closed && chord.len() > 1 {
                        chord.pop();
                        if !emitter.own.is_empty() {
                            emitter.own.pop();
                        }
                    }
                    if shape.segment_count() == 0
                        && let Some(only) = shape.vertices.first()
                    {
                        chord.clear();
                        chord.push(only.position);
                        if uses_own {
                            emitter.own = vec![only.height];
                        }
                    }
                    emitter.emit_path(&chord, shape.closed, color, width, linetype);
                }
                Geometry::Ellipse(shape) => {
                    emitter.own.clear();
                    chord = shape.tessellate(options.chord_tolerance);
                    if shape.is_full() && chord.len() > 1 {
                        chord.pop();
                    }
                    emitter.emit_path(&chord, shape.is_full(), color, width, linetype);
                }
                Geometry::Spline(shape) => {
                    emitter.own.clear();
                    chord = shape.tessellate(options.chord_tolerance);
                    emitter.emit_path(&chord, false, color, width, linetype);
                }
            }
        };
        if overlay {
            // Ascending, as for_each visits: the overlay is the same list, in
            // the same order, as a walk of the drawing that kept the selected.
            // SelectionSet::ids() is a copy, held here for the loop.
            let selected = if walk.is_some() {
                Vec::new()
            } else {
                selection.ids()
            };
            for &id in walk.unwrap_or(selected.as_slice()) {
                if let Some(entity) = model.entities.find(id) {
                    emit_one(entity);
                }
            }
        } else {
            model.entities.for_each(&mut emit_one);
        }
        let lowest = emitter.lowest;
        self.chord = chord;

        // What had no height and nothing under it goes on the datum, which
        // only now is known when the terrain did not decide it.
        let datum = if datum_known {
            datum_hint
        } else {
            lift_of(options).apply(if lowest.is_finite() {
                lowest
            } else if options.entity_elevation.is_finite() {
                options.entity_elevation
            } else {
                0.0
            })
        };
        for position in &mut out.positions[first_vertex..] {
            if position.z.is_nan() {
                position.z = datum;
            }
        }
        if let Some(lowest_height) = lowest_height {
            *lowest_height = lowest;
        }
    }
}

// ---- grid ------------------------------------------------------------------------

/// A grid sized to the scene, not a fixed 800 m patch: on a 12 km corridor
/// that was a stray square in empty space, and on a 20 m building it was a
/// sea of lines. Each line is emitted cell by cell. A full-length line's quad
/// touches every tile its bounding box does, and the fixed grid's 162 long
/// diagonals cost 70 to 95 ms a frame - 95% of a small scene's time; cut into
/// cells the same grid measured about 3 ms.
fn emit_grid(options: &SceneOptions<'_>, around: &AABB, z: f64, out: &mut DrawList) {
    let mut bounds = *around;
    if bounds.empty() || !bounds.min.is_finite() || !bounds.max.is_finite() {
        bounds = AABB::new(Vec3::new(-50.0, -50.0, 0.0), Vec3::new(50.0, 50.0, 0.0));
    }
    let span_x = bounds.max.x - bounds.min.x;
    let span_y = bounds.max.y - bounds.min.y;
    let span = cpp::max3(span_x, span_y, 1.0);
    let across = options.grid_cells_across.clamp(2, 100);
    let mut spacing = if options.grid_spacing.is_finite() && options.grid_spacing > 0.0 {
        options.grid_spacing
    } else {
        nice_step(span / f64::from(across))
    };
    // A reach of 15% past the scene on every side, so the model stands on the
    // grid rather than on its edge.
    let reach = 0.15 * span;
    // Bounded however the spacing was chosen: at most 200 cells a side.
    while (span_x + 2.0 * reach) / spacing > 200.0 || (span_y + 2.0 * reach) / spacing > 200.0 {
        spacing *= 2.0;
    }
    let i0 = ((bounds.min.x - reach) / spacing).floor() as i64;
    let i1 = ((bounds.max.x + reach) / spacing).ceil() as i64;
    let j0 = ((bounds.min.y - reach) / spacing).floor() as i64;
    let j1 = ((bounds.max.y + reach) / spacing).ceil() as i64;
    let cx = 0.5 * ((i0 + i1) as f64 * spacing);
    let cy = 0.5 * ((j0 + j1) as f64 * spacing);
    let radius = 0.5 * ((i1 - i0) as f64 * spacing).hypot((j1 - j0) as f64 * spacing);
    let scale = max_f32(options.pixel_scale, 0.1);

    // Faded towards the view's background with distance from the middle, so
    // the grid has no hard rim to compete with the model's edge.
    let faded = |color: Rgba, x: f64, y: f64| {
        let r = (x - cx).hypot(y - cy) / cpp::max(radius, 1.0e-9);
        mix(
            color,
            options.grid_fade_color,
            cpp::clamp((r - 0.45) / 0.55, 0.0, 1.0),
        )
    };
    let mut cell_line = |xa: f64, ya: f64, xb: f64, yb: f64, color: Rgba, width: f32| {
        let a = out.add_vertex(Vec3::new(xa, ya, z), faded(color, xa, ya));
        let b = out.add_vertex(Vec3::new(xb, yb, z), faded(color, xb, yb));
        out.add_line(a, b, width * scale, 0.0);
    };
    // Only where the REAL origin is in range: survey data at MGA coordinates
    // is kilometres from it, and the old axes through the scene's centre
    // looked like axes but meant nothing.
    let line_style = |index: i64, axis: Rgba| -> (Rgba, f32) {
        if index == 0 {
            return (axis, 2.0);
        }
        (
            if index % 5 == 0 {
                options.grid_major_color
            } else {
                options.grid_color
            },
            1.0,
        )
    };
    // The grid writes no depth (render_layers), so where two of its lines
    // cross, the one drawn later covers the other: the plain lines first,
    // then every fifth, then the axes, so that no plain line breaks an axis.
    let rank_of = |index: i64| {
        if index == 0 {
            2
        } else if index % 5 == 0 {
            1
        } else {
            0
        }
    };
    for rank in 0..3 {
        for i in i0..=i1 {
            if rank_of(i) != rank {
                continue;
            }
            let x = i as f64 * spacing;
            let (color, width) = line_style(i, options.axis_color_y);
            for j in j0..j1 {
                cell_line(
                    x,
                    j as f64 * spacing,
                    x,
                    (j + 1) as f64 * spacing,
                    color,
                    width,
                );
            }
        }
        for j in j0..=j1 {
            if rank_of(j) != rank {
                continue;
            }
            let y = j as f64 * spacing;
            let (color, width) = line_style(j, options.axis_color_x);
            for i in i0..i1 {
                cell_line(
                    i as f64 * spacing,
                    y,
                    (i + 1) as f64 * spacing,
                    y,
                    color,
                    width,
                );
            }
        }
    }
}

// ---- one frame -------------------------------------------------------------------

/// One frame of `layers` as the 3D view draws it: the depth range fitted to
/// the layers, the grid and the selection (whose ghosts may lie outside the
/// rest), the edges faded for `camera` (fade_edges), then one pass per layer
/// in declaration order into one depth buffer, each with its own depth rule:
///
/// ```text
/// grid       a backdrop, writing no depth, so the model always covers it.
///            It stands on the datum, exactly in the plane of a surface that
///            is flat at its lowest (a pad, a pond floor), where the
///            surface's slope push lost to it: the whole grid showed through
///            a flat pad.
/// terrain    written.
/// edges      tested against the terrain but not written. An edge and a line
///            draped across it both lie in the surface, each at one depth
///            across its width; off their centres those differ by up to a
///            pixel of the surface's slope, more than the half a footprint
///            between their biases at a low angle, so the edge drawn first
///            broke every draped line it crossed.
/// entities   written.
/// selection_casing
///            tested but not written, so the core drawn next covers it
///            wherever the two overlap, whatever either's pull.
/// selection  written.
/// ```
///
/// `options.clear` is honoured by the first pass only. Stops at, and returns,
/// the first failure.
pub fn render_layers(
    layers: &mut SceneLayers,
    camera: &mut Camera,
    rasterizer: &mut Rasterizer,
    target: &mut Framebuffer,
    options: RenderOptions<'_>,
) -> Result<RenderStats> {
    // The depth range is fitted to what is drawn EVERY frame: after an orbit,
    // a pan or a zoom it was left where frame() put it, and eight wheel
    // notches out pushed the model past the far plane.
    let _ = camera.fit_depth_range(scene_depth_box(layers));
    let draw_edges = SceneBuilder::fade_edges(layers, camera);

    // In this order into one depth buffer, so equal depths resolve the same
    // way every frame (the first drawn wins, Rule 7). Why the grid, the edges
    // and the selection's casing write no depth: render_layers' comment.
    let passes: [(Option<&DrawList>, bool); 6] = [
        (Some(&layers.grid), false),
        (Some(&layers.terrain), true),
        (draw_edges.then_some(&layers.edges), false),
        (Some(&layers.entities), true),
        (Some(&layers.selection_casing), false),
        (Some(&layers.selection), true),
    ];
    let mut options = options;
    let mut total = RenderStats::default();
    for (list, depth_write) in passes {
        // An empty list still clears when it comes first.
        let Some(list) = list else {
            continue;
        };
        if list.empty() && !options.clear {
            continue;
        }
        options.depth_write = depth_write;
        let result = rasterizer.render(list, camera, target, &options)?;
        total.vertices += result.vertices;
        total.triangles_submitted += result.triangles_submitted;
        total.triangles_rasterised += result.triangles_rasterised;
        total.lines_submitted += result.lines_submitted;
        total.points_submitted += result.points_submitted;
        total.fragments += result.fragments;
        total.bin_entries += result.bin_entries;
        total.tiles = result.tiles;
        options.clear = false;
    }
    Ok(total)
}

/// The box a frame's depth range is fitted to: every layer's (`bounds`), the
/// grid's, which reaches past them, and the selection's ghosts, which lie
/// where a view draws nothing. The one box render_layers, the GPU view's frame
/// and origin, and how deep the zoom reaches off the model (scene_zoom.rs,
/// zoom_anchor) all go by.
pub fn scene_depth_box(layers: &SceneLayers) -> AABB {
    let mut bounds = layers.bounds;
    bounds.expand(layers.grid.bounds());
    // A ghost of the selection lies where the view draws nothing, so outside
    // `bounds` (SceneLayers::bounds).
    bounds.expand(layers.selection.bounds());
    bounds
}

/// The world-space box of everything the scene would draw, with any vertical
/// exaggeration applied: linework spans from the datum to the highest height
/// or surface it can be put at. What a viewport frames when asked to zoom to
/// extents.
pub fn scene_bounds(
    document: &Document,
    surfaces: &[SceneSurface<'_>],
    options: &SceneOptions<'_>,
    meshes: &[SceneMesh<'_>],
) -> AABB {
    let lift = lift_of(options);
    let mut bounds = AABB::default();
    let mut terrain_low = f64::INFINITY;
    let mut terrain_high = f64::NEG_INFINITY;
    for item in surfaces {
        let Some(surface) = drawable_surface(item) else {
            continue;
        };
        let plan = surface.bounds();
        if plan.empty() {
            continue;
        }
        bounds.expand(Vec3::new(
            plan.min.x,
            plan.min.y,
            lift.apply(surface.min_elevation()),
        ));
        bounds.expand(Vec3::new(
            plan.max.x,
            plan.max.y,
            lift.apply(surface.max_elevation()),
        ));
        terrain_low = cpp::min(terrain_low, surface.min_elevation());
        terrain_high = cpp::max(terrain_high, surface.max_elevation());
    }
    for item in meshes {
        let Some(mesh) = drawable_mesh(item) else {
            continue;
        };
        let space = mesh.bounds();
        if space.empty() {
            continue;
        }
        bounds.expand(Vec3::new(space.min.x, space.min.y, lift.apply(space.min.z)));
        bounds.expand(Vec3::new(space.max.x, space.max.y, lift.apply(space.max.z)));
        terrain_low = cpp::min(terrain_low, space.min.z);
        terrain_high = cpp::max(terrain_high, space.max.z);
    }
    if options.draw_entities {
        // What is DRAWN, not every entity: a hidden stray far away used to
        // frame an almost empty view (audit REN-03). The same box a plan
        // view's Zoom Extents frames, from the one function that computes it.
        let model = document.model();
        let view = view_of(options);
        let plan = drawn_extent(model, view);
        if !plan.empty() {
            // In z the linework spans what the build can put it at: its own
            // heights, the surfaces it drapes on, and the datum.
            let mut low = terrain_low;
            let mut high = terrain_high;
            if options.linework == LineworkHeights::Heights
                || options.linework == LineworkHeights::HeightsThenDrape
            {
                model.entities.for_each(|entity| {
                    if !is_drawn(model, entity, view) {
                        return;
                    }
                    // As many heights as emit_entities reads for the kind:
                    // one for a point, text or circle, one per vertex of a
                    // line or polyline; an arc and a dimension are draped.
                    let count = match &entity.geometry {
                        Geometry::Polyline(line) => line.vertices.len(),
                        Geometry::Line(_) => 2,
                        Geometry::Arc(_) | Geometry::Dimension(_) => 0,
                        _ => 1,
                    };
                    for height in heights_of(&entity.properties, count).into_iter().flatten() {
                        if height.is_finite() {
                            low = cpp::min(low, height);
                            high = cpp::max(high, height);
                        }
                    }
                });
            }
            if !low.is_finite() {
                low = if options.entity_elevation.is_finite() {
                    options.entity_elevation
                } else {
                    0.0
                };
            }
            if !high.is_finite() {
                high = low;
            }
            bounds.expand(Vec3::new(plan.min.x, plan.min.y, lift.apply(low)));
            bounds.expand(Vec3::new(plan.max.x, plan.max.y, lift.apply(high)));
        }
    }
    bounds
}
