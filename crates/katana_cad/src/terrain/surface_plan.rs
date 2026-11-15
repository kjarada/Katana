//! A surface in the plan view, in its display style (docs/surface_display.md;
//! include/katana/cad/terrain/surface_plan.hpp,
//! src/katana_cad/terrain/surface_plan.cpp): what the plan painter lays down
//! and strokes for ONE surface seen through ONE view. Culled to what the view
//! shows and drawn to a level of detail, so that a 400 000-triangle surface
//! pans as a drawing does. The plan view, a plot and every sheet viewport draw
//! a surface through it, beneath the drawing's entities, in this order:
//!
//! ```text
//! faces       an IMAGE of the part of the surface in view: each pixel the
//!             colour of the surface under its centre - flat, by elevation
//!             (smooth, or banded exactly to the pixel), by slope or by
//!             aspect - at the style's opacity. An image rather than a
//!             polygon a triangle: zoomed out, a dense TIN has many triangles
//!             to a pixel, and a polygon each would be most of a frame for
//!             colour nobody can see.
//! triangles   each edge once, where it is at least PLAN_EDGE_PIXELS long in
//!             the view. A triangle whose every edge is shorter is not
//!             stroked at all but tinted into the image: too dense to show
//!             its edges, as the 3D view fades them out below the same size.
//! contours    traced once per surface and contour settings and kept
//!             (SurfacePlanCache), culled to the view, thinned to the pixel;
//!             the minors left out where they would lie closer than
//!             PLAN_CONTOUR_PIXELS apart, and the majors likewise.
//! breaklines  the constrained edges inside the surface, joined into lines.
//! boundary    the rim of the surface - its outside and its holes - as rings.
//! points      the vertices: marks where they lie PLAN_POINT_PIXELS apart or
//!             more, else single pixels in the image.
//! labels      the level along each major contour, spaced along it.
//! ```
//!
//! Plain data in, plain data out, no window: what is drawn is decided here,
//! where it is tested, and the painter only draws it.

use std::sync::{Arc, Mutex, PoisonError};

use katana_core::task_pool::TaskPool;
use katana_core::text::reply_quoted;
use katana_entity::Color;
use katana_geometry::polygon::{PolylineClip, PolylineRuns, clip_polyline_with_mode};
use katana_geometry::{Box2, Point2, Segment2};
use katana_math::cpp;
use katana_math::numerics::tolerance;
use katana_terrain::contours::contours;
use katana_terrain::{NO_TRIANGLE, NamedSurface, SurfaceStore, TinSurface};

use crate::terrain::surface_colouring::{
    SurfaceColouring, SurfaceLegend, aspect_sector, aspect_sector_colour, format_legend_value,
    legend_decimals, surface_colouring, surface_colouring_for, surface_legend,
};
use crate::terrain::surface_style::{
    SurfaceContours, SurfaceDisplayStyle, SurfaceFaces, SurfaceStyleSet, style_of,
};

// ---- the level of detail, in pixels of the view ------------------------------------------
//
// A pixel of the view is a logical pixel on screen and, on paper,
// PLAN_PAPER_PIXEL_MILLIMETRES of the page (the painter converts).

/// An edge shorter than this is not stroked. The 3D view's edges are gone
/// below the same 4 px (SceneBuilder::fade_edges, docs/render.md "Surfaces"):
/// a mesh of one-pixel lines four pixels apart already inks half its area and
/// reads as a sheet, not as triangles.
pub const PLAN_EDGE_PIXELS: f64 = 4.0;
/// A triangle too dense to stroke is tinted in the triangle colour at this
/// opacity: about what its edges would ink at the threshold (two one-pixel
/// edges a triangle across four pixels: 2 x 4 / (4 x 4 / 2) is all of it; a
/// third of that keeps the faces beneath readable).
pub const PLAN_DENSE_TRIANGLE_ALPHA: f64 = 0.35;
/// Two contours of one kind closer than this on average are not drawn: a band
/// of lines that close is ink, and the majors (or the faces) say more.
pub const PLAN_CONTOUR_PIXELS: f64 = 3.0;
/// Vertices closer than this on average are pixels in the image, not marks.
pub const PLAN_POINT_PIXELS: f64 = 4.0;
/// A drawn line keeps no vertex nearer than this to the one kept before it.
pub const PLAN_SIMPLIFY_PIXELS: f64 = 0.5;
/// Contour labels: one every PLAN_LABEL_SPACING_PIXELS along a major contour,
/// none on a contour shorter than PLAN_LABEL_MINIMUM_PIXELS, kept
/// PLAN_LABEL_MARGIN_PIXELS inside the view, and at most PLAN_MAXIMUM_LABELS a
/// surface a paint (more could not be read).
pub const PLAN_LABEL_SPACING_PIXELS: f64 = 360.0;
pub const PLAN_LABEL_MINIMUM_PIXELS: f64 = 90.0;
pub const PLAN_LABEL_MARGIN_PIXELS: f64 = 12.0;
pub const PLAN_MAXIMUM_LABELS: usize = 400;
/// Contours whose tracing would make more points than this are refused for
/// the plan, saying why (SurfacePlan::problem): an interval far too fine for
/// the relief would fill memory and the view with ink. Estimated, before
/// tracing, from how many levels cross each triangle.
pub const PLAN_MAXIMUM_CONTOUR_POINTS: usize = 4_000_000;
/// On paper a view pixel is this much of the page: a screen pixel's size at
/// 96 dpi, so a plot keeps the level of detail the screen showed at the same
/// scale.
pub const PLAN_PAPER_PIXEL_MILLIMETRES: f64 = 25.4 / 96.0;

// ---- how it is looked at -----------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct SurfacePlanView<'a> {
    /// The part of the model the view shows (the plan painter's visible box):
    /// what is culled to.
    pub visible: Box2,
    /// Model units per pixel of the view: what the level of detail measures.
    pub units_per_pixel: f64,
    /// The faces' image (plan_image_grid): the model box it covers and its
    /// size in pixels. No pixels: no image, and what would go into one -
    /// faces, dense triangles, dense points - is not drawn.
    pub image_box: Box2,
    pub image_width: i32,
    pub image_height: i32,
    pub label_spacing_pixels: f64,
    /// The threads the image is painted on; None: TaskPool::shared(). The
    /// image is the same at any count.
    pub pool: Option<&'a TaskPool>,
}

impl Default for SurfacePlanView<'_> {
    fn default() -> Self {
        Self {
            visible: Box2::default(),
            units_per_pixel: 1.0,
            image_box: Box2::default(),
            image_width: 0,
            image_height: 0,
            label_spacing_pixels: PLAN_LABEL_SPACING_PIXELS,
            pool: None,
        }
    }
}

/// The pixel grid of a surface's image: `bounds` (the surface's plan box) as
/// far as `visible` shows it, in pixels `units_per_pixel` wide lined up with
/// the visible box's top-left corner - the view's first pixel, so on an
/// unturned screen each image pixel is a device pixel - and coarsened evenly
/// when it would be more than `pixel_cap` pixels. Empty when they do not meet.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SurfacePlanImageGrid {
    pub r#box: Box2,
    pub width: i32,
    pub height: i32,
}

// ---- what is drawn -----------------------------------------------------------------------

/// Premultiplied 0xAARRGGBB, row 0 at the top (the box's largest y): the
/// layout of a premultiplied ARGB32 image, so the painter wraps it without a
/// copy.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SurfacePlanImage {
    pub r#box: Box2,
    pub width: i32,
    pub height: i32,
    pub pixels: Vec<u32>,
}

impl SurfacePlanImage {
    pub fn empty(&self) -> bool {
        self.width <= 0 || self.height <= 0 || self.pixels.is_empty()
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SurfacePlanContour {
    pub level: f64,
    pub major: bool,
    /// A run of the contour as far as the view shows it, thinned; a closed
    /// contour seen whole repeats its first point at its end.
    pub points: Vec<Point2>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SurfacePlanLabel {
    pub at: Point2,
    /// Of the contour there, counter-clockwise from +x in the model. The
    /// painter turns the text upright on the device.
    pub angle: f64,
    pub text: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SurfacePlanStats {
    pub triangles_in_view: usize,
    pub edges_stroked: usize,
    /// Not stroked: tinted into the image (PLAN_EDGE_PIXELS).
    pub triangles_tinted: usize,
    /// Pixels of the image the surface covers.
    pub image_pixels: usize,
    pub pixels_covered: usize,
    /// Traced for the style, whether drawn or not.
    pub contour_levels: usize,
    pub contour_runs: usize,
    pub contour_vertices: usize,
    pub minor_contours_hidden: bool,
    pub major_contours_hidden: bool,
    pub labels: usize,
    /// As marks.
    pub points_marked: usize,
    /// As pixels.
    pub points_in_image: usize,
    pub boundary_runs: usize,
    pub breakline_runs: usize,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SurfacePlan {
    pub name: String,
    pub image: SurfacePlanImage,
    pub triangle_colour: Color,
    pub triangles: Vec<Segment2>,
    pub minor_colour: Color,
    pub major_colour: Color,
    /// Minors first, then majors.
    pub contours: Vec<SurfacePlanContour>,
    pub breakline_colour: Color,
    pub breaklines: Vec<Vec<Point2>>,
    pub boundary_colour: Color,
    pub boundary: Vec<Vec<Point2>>,
    pub point_colour: Color,
    pub points: Vec<Point2>,
    pub labels: Vec<SurfacePlanLabel>,
    /// What the faces' colours mean, when the style colours them.
    pub legend: Option<SurfaceLegend>,
    pub stats: SurfacePlanStats,
    /// Why part of the style was not drawn (contours refused); empty when all
    /// of it was.
    pub problem: String,
}

// ---- why a style could not be drawn --------------------------------------------------------

/// Why part of one surface's style was not drawn, and whose it is: what a
/// paint reports so that the plan view, a plot and a sheet viewport all say
/// it, what a plot's reply carries and what a preflight finding is made of.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SurfaceProblem {
    /// The drawing's name for it.
    pub surface: String,
    /// The display style it is drawn in.
    pub style: String,
    /// "contours: an interval of 0.000500 crosses ... set a larger interval".
    pub reason: String,
}

impl SurfaceProblem {
    /// One line for a person or a log: `ground (style Fine): contours: ...`.
    pub fn text(&self) -> String {
        format!("{} (style {}): {}", self.surface, self.style, self.reason)
    }

    /// One record for a script or an agent, as every reply is:
    /// `warning surface="ground" style="Fine" reason="contours: ..."`.
    pub fn record(&self) -> String {
        format!(
            "warning surface={} style={} reason={}",
            reply_quoted(&self.surface),
            reply_quoted(&self.style),
            reply_quoted(&self.reason)
        )
    }
}

/// A visible surface whose style cannot be drawn in whole, with where it lies
/// in plan, so that the plans which show it can be told.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SurfaceStyleProblem {
    pub problem: SurfaceProblem,
    pub bounds: Box2,
}

// ---- what is kept, per surface -------------------------------------------------------------

/// A line drawn in plan, worked out once: the rim, a run of breaklines, a
/// contour.
#[derive(Clone, Debug, Default)]
struct KeptLine {
    points: Vec<Point2>,
    closed: bool,
    bounds: Box2,
    length: f64,
    /// A contour's.
    level: f64,
    /// A contour's.
    major: bool,
}

#[derive(Clone, Debug, Default)]
struct ContourSet {
    interval: f64,
    base: f64,
    major_every: i32,
    levels: usize,
    lines: Vec<KeptLine>,
    problem: String,
}

/// The cull grid's shape: a uniform grid over the surface's plan box.
#[derive(Clone, Copy, Debug)]
struct GridShape {
    min_x: f64,
    min_y: f64,
    cell_width: f64,
    cell_height: f64,
    columns: u32,
    rows: u32,
}

impl GridShape {
    fn column(&self, x: f64) -> u32 {
        cpp::clamp(
            ((x - self.min_x) / self.cell_width).floor(),
            0.0,
            f64::from(self.columns) - 1.0,
        ) as u32
    }

    fn row(&self, y: f64) -> u32 {
        cpp::clamp(
            ((y - self.min_y) / self.cell_height).floor(),
            0.0,
            f64::from(self.rows) - 1.0,
        ) as u32
    }

    /// The columns and rows `bounds` reaches: c0, c1, r0, r1.
    fn range(&self, bounds: &Box2) -> [u32; 4] {
        [
            self.column(bounds.min.x),
            self.column(bounds.max.x),
            self.row(bounds.min.y),
            self.row(bounds.max.y),
        ]
    }

    fn cell(&self, row: u32, column: u32) -> usize {
        row as usize * self.columns as usize + column as usize
    }
}

#[derive(Debug)]
struct SurfaceEntry {
    surface: Arc<TinSurface>,

    /// The cull grid: a uniform grid over the surface's plan box, each cell
    /// listing the triangles whose box reaches into it (CSR, ascending).
    grid: GridShape,
    cell_start: Vec<u32>,
    cell_triangles: Vec<u32>,

    /// The area-weighted mean slope (rise over run) and the steepest face in
    /// percent; the mean distance between vertices.
    mean_slope: f64,
    steepest_percent: f64,
    vertex_spacing: f64,

    lines_built: bool,
    rim: Vec<KeptLine>,
    breaklines: Vec<KeptLine>,

    contours: Option<ContourSet>,
}

fn line_box(points: &[Point2]) -> Box2 {
    let mut bounds = Box2::default();
    for &p in points {
        bounds.expand(p);
    }
    bounds
}

fn line_length(points: &[Point2], closed: bool) -> f64 {
    let mut length = 0.0;
    for pair in points.windows(2) {
        length += pair[0].distance_to(pair[1]);
    }
    if closed && points.len() > 2 {
        length += points[points.len() - 1].distance_to(points[0]);
    }
    length
}

fn triangle_box(surface: &TinSurface, t: usize) -> Box2 {
    let corners = surface.triangles()[t];
    let vertices = surface.vertices();
    let mut bounds = Box2::default();
    for v in corners {
        let vertex = vertices[v as usize];
        bounds.expand(Point2::new(vertex.x, vertex.y));
    }
    bounds
}

/// The entry for `surface` with its cull grid, mean slope and spacing.
fn build_grid(surface: &Arc<TinSurface>) -> SurfaceEntry {
    let count = surface.triangle_count();
    let grid_box = *surface.bounds();
    // About four triangles a cell, at most 1024 cells a side: a query for a
    // view then visits about the triangles in it, and the grid costs two
    // integers a triangle and one a cell.
    let width = cpp::max(grid_box.width(), tolerance::GEOMETRIC);
    let height = cpp::max(grid_box.height(), tolerance::GEOMETRIC);
    let cells = cpp::max(1.0, count as f64 / 4.0);
    let aspect = width / height;
    let columns = cpp::clamp((cells * aspect).sqrt().ceil(), 1.0, 1024.0) as u32;
    let rows = cpp::clamp((cells / f64::from(columns)).ceil(), 1.0, 1024.0) as u32;
    let grid = GridShape {
        min_x: grid_box.min.x,
        min_y: grid_box.min.y,
        cell_width: width / f64::from(columns),
        cell_height: height / f64::from(rows),
        columns,
        rows,
    };

    let cell_count = columns as usize * rows as usize;
    let mut cell_start = vec![0u32; cell_count + 1];
    for t in 0..count {
        let [c0, c1, r0, r1] = grid.range(&triangle_box(surface, t));
        for r in r0..=r1 {
            for c in c0..=c1 {
                cell_start[grid.cell(r, c) + 1] += 1;
            }
        }
    }
    for i in 0..cell_count {
        cell_start[i + 1] += cell_start[i];
    }
    let mut cell_triangles = vec![0u32; cell_start[cell_count] as usize];
    let mut fill = cell_start[..cell_count].to_vec();
    for t in 0..count {
        let [c0, c1, r0, r1] = grid.range(&triangle_box(surface, t));
        for r in r0..=r1 {
            for c in c0..=c1 {
                let slot = &mut fill[grid.cell(r, c)];
                cell_triangles[*slot as usize] = t as u32;
                *slot += 1;
            }
        }
    }

    let mut weighted = 0.0;
    let mut area = 0.0;
    let mut steepest_percent = 0.0;
    for t in 0..count {
        let Some(slope) = surface.triangle_slope_aspect(t) else {
            continue;
        };
        if !slope.slope.is_finite() {
            continue;
        }
        let a = surface.triangle_plan_area(t);
        weighted += slope.slope * a;
        area += a;
        steepest_percent = cpp::max(steepest_percent, slope.slope * 100.0);
    }
    let vertex_spacing = if surface.vertex_count() > 0 {
        (surface.plan_area() / surface.vertex_count() as f64).sqrt()
    } else {
        0.0
    };
    SurfaceEntry {
        surface: Arc::clone(surface),
        grid,
        cell_start,
        cell_triangles,
        mean_slope: if area > 0.0 { weighted / area } else { 0.0 },
        steepest_percent,
        vertex_spacing,
        lines_built: false,
        rim: Vec::new(),
        breaklines: Vec::new(),
        contours: None,
    }
}

/// Where the ends of edges at `vertex` lie in `ends` (sorted): [lo, hi).
fn end_range(ends: &[(u32, u32)], vertex: u32) -> (usize, usize) {
    let lo = ends.partition_point(|end| *end < (vertex, 0));
    let mut hi = lo;
    while hi < ends.len() && ends[hi].0 == vertex {
        hi += 1;
    }
    (lo, hi)
}

/// One line from `start` along `first_edge`, on through every vertex two
/// edges meet at.
fn walk_edges(
    start: u32,
    first_edge: u32,
    edges: &[(u32, u32)],
    ends: &[(u32, u32)],
    used: &mut [bool],
    surface: &TinSurface,
) -> KeptLine {
    let vertices = surface.vertices();
    let plan = |vertex: u32| {
        let at = vertices[vertex as usize];
        Point2::new(at.x, at.y)
    };
    let mut line = KeptLine::default();
    let mut at = start;
    let mut edge = first_edge as usize;
    line.points.push(plan(at));
    loop {
        used[edge] = true;
        let next = if edges[edge].0 == at {
            edges[edge].1
        } else {
            edges[edge].0
        };
        line.points.push(plan(next));
        at = next;
        let (lo, hi) = end_range(ends, at);
        // Go on only through a vertex two edges meet at: three or more is
        // where runs part, and each is a line of its own.
        if hi - lo != 2 {
            break;
        }
        match ends[lo..hi].iter().find(|end| !used[end.1 as usize]) {
            None => break,
            Some(end) => edge = end.1 as usize,
        }
    }
    if line.points.len() > 2 && at == start {
        line.points.pop();
        line.closed = true;
    }
    line.bounds = line_box(&line.points);
    line.length = line_length(&line.points, line.closed);
    line
}

/// Joins edges (vertex pairs) into lines: from each end of a run (a vertex met
/// by other than two edges) along to the next, then whatever closed loops are
/// left. In vertex order, so the same edges always give the same lines.
fn join_edges(surface: &TinSurface, edges: Vec<(u32, u32)>) -> Vec<KeptLine> {
    let mut lines = Vec::new();
    if edges.is_empty() {
        return lines;
    }
    // Each edge from both ends, by vertex.
    let mut ends: Vec<(u32, u32)> = Vec::with_capacity(2 * edges.len());
    for (i, edge) in edges.iter().enumerate() {
        ends.push((edge.0, i as u32));
        ends.push((edge.1, i as u32));
    }
    // Every pair is distinct (an edge's index is), so the order is total.
    ends.sort_unstable();
    let mut used = vec![false; edges.len()];
    // Runs first, from each vertex not met by exactly two edges.
    let mut i = 0;
    while i < ends.len() {
        let vertex = ends[i].0;
        let (lo, hi) = end_range(&ends, vertex);
        if hi - lo != 2 {
            for &(_, edge) in &ends[lo..hi] {
                if !used[edge as usize] {
                    lines.push(walk_edges(vertex, edge, &edges, &ends, &mut used, surface));
                }
            }
        }
        i = hi;
    }
    // Then the loops.
    for &(vertex, edge) in &ends {
        if !used[edge as usize] {
            lines.push(walk_edges(vertex, edge, &edges, &ends, &mut used, surface));
        }
    }
    lines
}

fn build_lines(entry: &mut SurfaceEntry) {
    let surface: &TinSurface = &entry.surface;
    let mut rim = Vec::new();
    let mut breaklines = Vec::new();
    for t in 0..surface.triangle_count() {
        let corners = surface.triangles()[t];
        for e in 0..3 {
            let a = corners[e];
            let b = corners[(e + 1) % 3];
            let across = surface.neighbors()[t][e];
            if across == NO_TRIANGLE {
                rim.push((a, b));
            } else if across as usize > t && surface.is_edge_constrained(t, e) {
                // Once, from the lower-numbered triangle of the two.
                breaklines.push((a, b));
            }
        }
    }
    let rim = join_edges(surface, rim);
    let breaklines = join_edges(surface, breaklines);
    entry.rim = rim;
    entry.breaklines = breaklines;
    entry.lines_built = true;
}

/// How many contour points tracing at these settings would make: one a level
/// crossing a triangle, near enough (a crossing is two points shared with the
/// triangles either side).
fn estimate_contour_points(surface: &TinSurface, interval: f64, base: f64) -> usize {
    let mut points = 0usize;
    let vertices = surface.vertices();
    for corners in surface.triangles() {
        let z = |k: usize| vertices[corners[k] as usize].z;
        let lo = cpp::min3(z(0), z(1), z(2));
        let hi = cpp::max3(z(0), z(1), z(2));
        let crossed = ((hi - base) / interval).floor() - ((lo - base) / interval).floor();
        if crossed > 0.0 {
            points += cpp::min(crossed, 1.0e9) as usize;
        }
        if points > PLAN_MAXIMUM_CONTOUR_POINTS {
            break;
        }
    }
    points
}

fn trace_contours(surface: &TinSurface, settings: &SurfaceContours) -> ContourSet {
    let mut set = ContourSet {
        interval: settings.interval,
        base: settings.base,
        major_every: settings.major_every,
        problem: contour_refusal(surface, settings),
        ..ContourSet::default()
    };
    if !set.problem.is_empty() {
        return set;
    }
    let traced = match contours(
        surface,
        settings.interval,
        settings.base,
        std::cmp::max(settings.major_every, 0) as usize,
        None,
    ) {
        Ok(traced) => traced,
        Err(error) => {
            set.problem = format!("contours: {}", error.describe());
            return set;
        }
    };
    let mut previous = f64::NAN;
    for contour in traced {
        if contour.elevation != previous {
            set.levels += 1;
            previous = contour.elevation;
        }
        let bounds = line_box(&contour.line.vertices);
        let length = line_length(&contour.line.vertices, contour.line.closed);
        set.lines.push(KeptLine {
            points: contour.line.vertices,
            closed: contour.line.closed,
            bounds,
            length,
            level: contour.elevation,
            major: contour.major,
        });
    }
    set
}

/// Keeps a vertex only when it is at least `tolerance` from the one kept
/// before it, and always the last: radial-distance thinning, which never
/// moves a vertex and never drops a line's ends.
fn thin_into(points: &[Point2], tolerance: f64, out: &mut Vec<Point2>) {
    out.clear();
    let Some(&first) = points.first() else {
        return;
    };
    out.push(first);
    let squared = tolerance * tolerance;
    let mut last_kept = first;
    if points.len() > 2 {
        for &p in &points[1..points.len() - 1] {
            let dx = p.x - last_kept.x;
            let dy = p.y - last_kept.y;
            if dx * dx + dy * dy >= squared {
                out.push(p);
                last_kept = p;
            }
        }
    }
    if points.len() > 1 {
        out.push(points[points.len() - 1]);
    }
}

/// `line` as far as `bounds` shows it, thinned: the whole line when the box
/// holds it (a closed one repeating its first point), else its runs of whole
/// segments that reach the box (geometry's clip_polyline).
fn visible_runs(
    line: &KeptLine,
    bounds: &Box2,
    tolerance: f64,
    scratch: &mut PolylineRuns,
    closed_scratch: &mut Vec<Point2>,
    mut emit: impl FnMut(Vec<Point2>),
) {
    if line.points.len() < 2 || !line.bounds.intersects(*bounds) {
        return;
    }
    let mut thinned = Vec::new();
    if bounds.contains_box(line.bounds) {
        if line.closed {
            closed_scratch.clear();
            closed_scratch.extend_from_slice(&line.points);
            closed_scratch.push(line.points[0]);
            thin_into(closed_scratch, tolerance, &mut thinned);
        } else {
            thin_into(&line.points, tolerance, &mut thinned);
        }
        if thinned.len() >= 2 {
            emit(thinned);
        }
        return;
    }
    clip_polyline_with_mode(
        &line.points,
        line.closed,
        bounds,
        scratch,
        PolylineClip::WholeSegments,
    );
    for run in 0..scratch.size() {
        let points = &scratch.points[scratch.begin(run)..scratch.ends[run]];
        thin_into(points, tolerance, &mut thinned);
        if thinned.len() >= 2 {
            emit(std::mem::take(&mut thinned));
        }
    }
}

// ---- the faces' image ----------------------------------------------------------------------

fn premultiplied(colour: Color, opacity: f64) -> u32 {
    let alpha = cpp::clamp(opacity, 0.0, 1.0) * (f64::from(colour.a) / 255.0);
    // std::lround: half away from zero.
    let a = (alpha * 255.0).round() as u32;
    let channel = |c: u8| (u32::from(c) * a + 127) / 255;
    (a << 24) | (channel(colour.r) << 16) | (channel(colour.g) << 8) | channel(colour.b)
}

/// `top` over `under`, both premultiplied.
fn over(top: u32, under: u32) -> u32 {
    let inverse = 255 - (top >> 24);
    let channel = |shift: u32| {
        let t = (top >> shift) & 0xff;
        let u = (under >> shift) & 0xff;
        std::cmp::min(255, t + (u * inverse + 127) / 255) << shift
    };
    channel(24) | channel(16) | channel(8) | channel(0)
}

/// What one paint asks of the image, for every triangle.
#[derive(Clone, Copy)]
struct FaceRule<'c> {
    faces: SurfaceFaces,
    colouring: Option<&'c SurfaceColouring>,
    opacity: f64,
    /// Dense triangles are tinted.
    tint: bool,
    /// Premultiplied.
    tint_colour: u32,
}

/// Rows of the image a band of the paint covers, and how many of its pixels
/// were covered.
type BandRows<'p> = Mutex<(&'p mut [u32], usize)>;

/// One edge of a triangle in the image, as `rasterise` tests it.
#[derive(Clone, Copy, Default)]
struct Edge {
    /// From the canonical start, along it.
    ax: f64,
    ay: f64,
    dx: f64,
    dy: f64,
    /// The orientation times +1 (canonical) or -1.
    sign: f64,
    /// A centre exactly on it is this triangle's.
    owns: bool,
}

struct FaceImage<'s> {
    surface: &'s TinSurface,
    width: i32,
    height: i32,
    x0: f64,
    y0: f64,
    size: f64,
}

impl<'s> FaceImage<'s> {
    fn new(surface: &'s TinSurface, view: &SurfacePlanView<'_>) -> Self {
        Self {
            surface,
            width: view.image_width,
            height: view.image_height,
            x0: view.image_box.min.x,
            y0: view.image_box.max.y,
            size: view.image_box.width() / f64::from(std::cmp::max(view.image_width, 1)),
        }
    }

    fn pixel_x(&self, x: f64) -> f64 {
        (x - self.x0) / self.size
    }

    fn pixel_y(&self, y: f64) -> f64 {
        (self.y0 - y) / self.size
    }

    /// A pixel index from a double that may lie far outside [0, count).
    fn to_pixel(value: f64, count: i32) -> i32 {
        cpp::clamp(
            if value.is_finite() { value } else { 0.0 },
            0.0,
            f64::from(std::cmp::max(count - 1, 0)),
        ) as i32
    }

    /// Paints `triangles` (ascending) by `rule`; `dense` says which are
    /// tinted. In bands of rows across the pool: each band writes its own
    /// rows, and takes its triangles in ascending order, so the image is the
    /// same at any thread count.
    fn paint(
        &self,
        pixels: &mut [u32],
        triangles: &[u32],
        dense: &[bool],
        rule: &FaceRule<'_>,
        pool: &TaskPool,
    ) -> usize {
        const BAND_ROWS: i32 = 32;
        let height = self.height;
        let band_count = (height + BAND_ROWS - 1) / BAND_ROWS;
        // Bin each triangle into the bands of rows its pixel box reaches.
        let mut start = vec![0u32; band_count as usize + 1];
        let mut spans = vec![[0i32; 2]; triangles.len()];
        let vertices = self.surface.vertices();
        for (i, &t) in triangles.iter().enumerate() {
            let corners = self.surface.triangles()[t as usize];
            let mut top = f64::INFINITY;
            let mut bottom = f64::NEG_INFINITY;
            for v in corners {
                let y = self.pixel_y(vertices[v as usize].y);
                top = cpp::min(top, y);
                bottom = cpp::max(bottom, y);
            }
            // The rows whose centres (j + 0.5) the triangle can reach,
            // clamped as doubles first: zoomed far in, a corner can be more
            // pixels away than an int holds.
            let first = Self::to_pixel((top - 0.5).ceil(), height);
            let last = Self::to_pixel((bottom - 0.5).floor(), height);
            if !(top - 0.5 <= f64::from(height) - 1.0) || !(bottom - 0.5 >= 0.0) {
                spans[i] = [1, 0];
                continue;
            }
            spans[i] = [first, last];
            if first > last {
                continue;
            }
            for band in first / BAND_ROWS..=last / BAND_ROWS {
                start[band as usize + 1] += 1;
            }
        }
        for band in 0..band_count as usize {
            start[band + 1] += start[band];
        }
        let mut binned = vec![0u32; start[band_count as usize] as usize];
        let mut fill = start[..band_count as usize].to_vec();
        for (i, span) in spans.iter().enumerate() {
            if span[0] > span[1] {
                continue;
            }
            for band in span[0] / BAND_ROWS..=span[1] / BAND_ROWS {
                let slot = &mut fill[band as usize];
                binned[*slot as usize] = i as u32;
                *slot += 1;
            }
        }
        let width = self.width as usize;
        let bands: Vec<BandRows<'_>> = pixels
            .chunks_mut(BAND_ROWS as usize * width)
            .map(|rows| Mutex::new((rows, 0)))
            .collect();
        pool.parallel_ranges(0, band_count as usize, 1, |lo, hi| {
            for band in lo..hi {
                let mut guard = bands[band].lock().unwrap_or_else(PoisonError::into_inner);
                let (rows, covered) = &mut *guard;
                let row_lo = band as i32 * BAND_ROWS;
                let row_hi = std::cmp::min(height - 1, row_lo + BAND_ROWS - 1);
                for k in start[band]..start[band + 1] {
                    let i = binned[k as usize] as usize;
                    let first = std::cmp::max(row_lo, spans[i][0]);
                    let last = std::cmp::min(row_hi, spans[i][1]);
                    *covered += self.rasterise(
                        &mut rows[..],
                        row_lo,
                        triangles[i],
                        first,
                        last,
                        dense[i],
                        rule,
                    );
                }
            }
        });
        bands
            .into_iter()
            .map(|band| band.into_inner().unwrap_or_else(PoisonError::into_inner).1)
            .sum()
    }

    /// One pixel of `colour`, opaque, at a model point: a vertex too dense to
    /// mark. False when the point is off the image.
    fn dot(&self, pixels: &mut [u32], at: Point2, colour: u32) -> bool {
        let x = (at.x - self.x0) / self.size;
        let y = self.pixel_y(at.y);
        if !(x >= 0.0) || !(y >= 0.0) || x >= f64::from(self.width) || y >= f64::from(self.height) {
            return false;
        }
        pixels[y as usize * self.width as usize + x as usize] = colour;
        true
    }

    /// The pixels of triangle `t` whose centres it covers, in rows first to
    /// last (`rows` holds the image's rows from `row_offset` on). Each edge's
    /// function is worked out from its lower-numbered vertex to its higher,
    /// the same arithmetic in both triangles that share it, so a centre
    /// exactly on the edge (exactly zero) is a tie they both see, and the tie
    /// goes by the edge's direction in the image, never by vertex numbers:
    /// the top-left rule of the graphics APIs. A tie is the triangle's when
    /// its edge runs up the image (a left edge), or along it with the inside
    /// below (a top edge) - the inside being on the edge's positive side.
    /// That is the same as testing the centre moved by an infinitesimal
    /// (e, e^2), which no edge passes through, so a centre on an edge goes to
    /// one of the two triangles and one on a VERTEX to exactly one of those
    /// round it: no pinholes, nothing twice. (Deciding by vertex numbers
    /// settled an edge, but a vertex only for a fan whose centre was numbered
    /// between its neighbours: a vertex numbered below all of them, or above
    /// all, was no triangle's, and one whose neighbours alternated was two's.)
    #[allow(
        clippy::too_many_arguments,
        reason = "The C++ member's arguments, with the band's rows and where they start."
    )]
    fn rasterise(
        &self,
        rows: &mut [u32],
        row_offset: i32,
        t: u32,
        first: i32,
        last: i32,
        dense: bool,
        rule: &FaceRule<'_>,
    ) -> usize {
        if first > last {
            return 0;
        }
        let corners = self.surface.triangles()[t as usize];
        let vertices = self.surface.vertices();
        let mut px = [0.0; 3];
        let mut py = [0.0; 3];
        let mut pz = [0.0; 3];
        for (k, &corner) in corners.iter().enumerate() {
            let vertex = vertices[corner as usize];
            px[k] = self.pixel_x(vertex.x);
            py[k] = self.pixel_y(vertex.y);
            pz[k] = vertex.z;
        }
        let area2 = (px[1] - px[0]) * (py[2] - py[0]) - (py[1] - py[0]) * (px[2] - px[0]);
        if !(area2.abs() > 0.0) || !area2.is_finite() {
            return 0;
        }
        let orientation = if area2 > 0.0 { 1.0 } else { -1.0 };
        // Edge k runs from corner k to corner k + 1; its canonical way round
        // is from the lower vertex number to the higher.
        let mut edges = [Edge::default(); 3];
        for (k, edge) in edges.iter_mut().enumerate() {
            let from = k;
            let to = (k + 1) % 3;
            let forward = corners[from] < corners[to];
            let lo = if forward { from } else { to };
            let hi = if forward { to } else { from };
            let dx = px[hi] - px[lo];
            let dy = py[hi] - py[lo];
            let sign = orientation * if forward { 1.0 } else { -1.0 };
            // The edge run so the inside is on its positive side; the image's
            // y runs down. Up is a left edge, along with the inside below a
            // top edge. The neighbour across it sees the exact opposite.
            let up_x = sign * dx;
            let up_y = sign * dy;
            *edge = Edge {
                ax: px[lo],
                ay: py[lo],
                dx,
                dy,
                sign,
                owns: up_y < 0.0 || (up_y == 0.0 && up_x > 0.0),
            };
        }
        let edge_value = |edge: &Edge, x: f64, y: f64| {
            edge.sign * (edge.dx * (y - edge.ay) - edge.dy * (x - edge.ax))
        };
        let inside = |edge: &Edge, value: f64| value > 0.0 || (value == 0.0 && edge.owns);

        // The colour of the whole triangle, when it does not vary across it.
        let mut face_colour = 0u32;
        let mut per_pixel: Option<&SurfaceColouring> = None;
        if let Some(colouring) = rule.colouring {
            match rule.faces {
                SurfaceFaces::None => {}
                SurfaceFaces::Flat => face_colour = premultiplied(colouring.flat, rule.opacity),
                SurfaceFaces::Elevation => per_pixel = Some(colouring),
                SurfaceFaces::Slope | SurfaceFaces::Aspect => {
                    let slope = self.surface.triangle_slope_aspect(t as usize);
                    let mut colour = colouring.flat;
                    if let Some(slope) = slope {
                        if rule.faces == SurfaceFaces::Slope {
                            colour = colouring.colour_at(slope.slope * 100.0);
                        } else if let Some(aspect) = slope.aspect {
                            colour = aspect_sector_colour(aspect_sector(aspect));
                        }
                    }
                    face_colour = premultiplied(colour, rule.opacity);
                }
            }
        }
        let tinted = dense && rule.tint;
        if rule.faces == SurfaceFaces::None && !tinted {
            return 0;
        }
        let z_low = cpp::min3(pz[0], pz[1], pz[2]);
        let z_high = cpp::max3(pz[0], pz[1], pz[2]);
        let left = cpp::min3(px[0], px[1], px[2]);
        let right = cpp::max3(px[0], px[1], px[2]);
        if !(right - 0.5 >= 0.0) || !(left - 0.5 <= f64::from(self.width) - 1.0) {
            return 0;
        }
        let column_lo = Self::to_pixel((left - 0.5).ceil(), self.width);
        let column_hi = Self::to_pixel((right - 0.5).floor(), self.width);
        let width = self.width as usize;
        let mut covered = 0;
        for j in first..=last {
            let y = f64::from(j) + 0.5;
            // The row's span from each edge's line, widened by a pixel, then
            // every pixel in it tested exactly.
            let mut span_lo = f64::from(column_lo);
            let mut span_hi = f64::from(column_hi);
            let mut empty = false;
            for edge in &edges {
                // value(x) = a + b x along the row.
                let b = -edge.sign * edge.dy;
                let a = edge.sign * (edge.dx * (y - edge.ay) + edge.dy * edge.ax);
                if b > 0.0 {
                    span_lo = cpp::max(span_lo, (-a / b - 0.5).floor() - 1.0);
                } else if b < 0.0 {
                    span_hi = cpp::min(span_hi, (-a / b - 0.5).ceil() + 1.0);
                } else if a < 0.0 {
                    empty = true;
                }
            }
            if empty || span_lo > span_hi {
                continue;
            }
            let row_start = (j - row_offset) as usize * width;
            let row = &mut rows[row_start..row_start + width];
            for i in span_lo as i32..=span_hi as i32 {
                let x = f64::from(i) + 0.5;
                let v0 = edge_value(&edges[0], x, y);
                let v1 = edge_value(&edges[1], x, y);
                let v2 = edge_value(&edges[2], x, y);
                if !inside(&edges[0], v0) || !inside(&edges[1], v1) || !inside(&edges[2], v2) {
                    continue;
                }
                let mut colour = face_colour;
                if let Some(colouring) = per_pixel {
                    // The height at the pixel's centre: each corner weighted
                    // by the edge opposite it, the values summing to the
                    // doubled area. Held inside the corners' range, which a
                    // centre on an edge may round past.
                    let sum = v0 + v1 + v2;
                    let z = if sum > 0.0 {
                        cpp::clamp((v1 * pz[0] + v2 * pz[1] + v0 * pz[2]) / sum, z_low, z_high)
                    } else {
                        pz[0]
                    };
                    colour = premultiplied(colouring.colour_at(z), rule.opacity);
                }
                if tinted {
                    colour = if rule.faces == SurfaceFaces::None {
                        rule.tint_colour
                    } else {
                        over(rule.tint_colour, colour)
                    };
                }
                row[i as usize] = colour;
                covered += 1;
            }
        }
        covered
    }
}

// ---- what is kept between paints -----------------------------------------------------------

/// Per surface: its cull grid, its rim and breaklines joined into lines, its
/// mean slope, and its contours for the settings last drawn. Keyed on the
/// surface itself - shared and immutable, so an edited surface is a new one -
/// and kept alive while kept. One per view or plotting thread, never shared
/// between threads; a copy starts empty and works out again what it needs.
/// Every surface asked about is kept until retain() lets it go, however many:
/// the caller bounds the cache, to the drawing's surfaces (a paint calls
/// retain with them). A cap on the count would make a paint of one surface
/// more than the cap rebuild every surface every time, so there is none.
#[derive(Debug, Default)]
pub struct SurfacePlanCache {
    entries: Vec<SurfaceEntry>,
    tracings: usize,
    grids: usize,
}

/// A copy starts empty, as the C++'s copy does: what it keeps is worked out
/// again by whoever holds the copy.
impl Clone for SurfacePlanCache {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl SurfacePlanCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Keeps only the surfaces in `alive`: what a paint draws, so a surface
    /// removed from the drawing is let go.
    pub fn retain(&mut self, alive: &[&TinSurface]) {
        self.entries.retain(|entry| {
            alive
                .iter()
                .any(|&surface| std::ptr::eq(Arc::as_ptr(&entry.surface), surface))
        });
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn surface_count(&self) -> usize {
        self.entries.len()
    }

    /// How many times contours have been traced since construction: what
    /// proves a second paint reuses them.
    pub fn contour_tracings(&self) -> usize {
        self.tracings
    }

    /// How many cull grids have been built since construction.
    pub fn grids_built(&self) -> usize {
        self.grids
    }

    /// Every surface asked about is kept until retain() lets it go: the caller
    /// bounds the cache to the drawing. A count cap evicting the least
    /// recently used would be wrong for a paint, which asks for every visible
    /// surface in the same order each time: with one more surface than the
    /// cap the one wanted next is always the one just evicted, and every
    /// paint rebuilds every grid and traces every contour again (measured with
    /// 33 surfaces: 61-69 ms a repaint against 3-7 ms with 32).
    fn entry_for(&mut self, surface: &Arc<TinSurface>) -> usize {
        if let Some(index) = self
            .entries
            .iter()
            .position(|entry| Arc::ptr_eq(&entry.surface, surface))
        {
            return index;
        }
        self.entries.push(build_grid(surface));
        self.grids += 1;
        self.entries.len() - 1
    }
}

// ---- culling ---------------------------------------------------------------------------------

fn triangles_in_box(entry: &SurfaceEntry, bounds: &Box2, marks: &mut Vec<u64>) -> Vec<u32> {
    let surface: &TinSurface = &entry.surface;
    let count = surface.triangle_count();
    let mut found = Vec::new();
    marks.clear();
    marks.resize(count.div_ceil(64), 0);
    if count == 0 || bounds.empty() || !bounds.intersects(*surface.bounds()) {
        return found;
    }
    if bounds.contains_box(*surface.bounds()) {
        // Every triangle's box is inside the surface's, so all of them.
        found.extend(0..count as u32);
        marks.fill(!0u64);
        return found;
    }
    let [c0, c1, r0, r1] = entry.grid.range(bounds);
    for r in r0..=r1 {
        for c in c0..=c1 {
            let cell = entry.grid.cell(r, c);
            for &t in &entry.cell_triangles
                [entry.cell_start[cell] as usize..entry.cell_start[cell + 1] as usize]
            {
                let word = &mut marks[(t / 64) as usize];
                let bit = 1u64 << (t % 64);
                if *word & bit == 0 && triangle_box(surface, t as usize).intersects(*bounds) {
                    *word |= bit;
                }
            }
        }
    }
    // In ascending order, from the marks.
    for (w, &mark) in marks.iter().enumerate() {
        let mut word = mark;
        while word != 0 {
            let bit = word.trailing_zeros();
            found.push((w * 64 + bit as usize) as u32);
            word &= word - 1;
        }
    }
    found
}

fn marked(marks: &[u64], t: u32) -> bool {
    ((t / 64) as usize) < marks.len() && ((marks[(t / 64) as usize] >> (t % 64)) & 1) != 0
}

/// The triangles of `surface` whose plan box meets `bounds`, ascending: what a
/// paint culls the surface to, through the cache's grid. None: none.
pub fn plan_triangles_in_box(
    surface: Option<&Arc<TinSurface>>,
    bounds: &Box2,
    cache: &mut SurfacePlanCache,
) -> Vec<u32> {
    let Some(surface) = surface else {
        return Vec::new();
    };
    let mut marks = Vec::new();
    let index = cache.entry_for(surface);
    triangles_in_box(&cache.entries[index], bounds, &mut marks)
}

/// The same found by testing every triangle: what the grid is held to.
pub fn plan_triangles_in_box_exhaustive(surface: &TinSurface, bounds: &Box2) -> Vec<u32> {
    (0..surface.triangle_count())
        .filter(|&t| triangle_box(surface, t).intersects(*bounds))
        .map(|t| t as u32)
        .collect()
}

// ---- the grid of the image -------------------------------------------------------------------

pub fn plan_image_grid(
    visible: &Box2,
    bounds: &Box2,
    units_per_pixel: f64,
    pixel_cap: usize,
) -> SurfacePlanImageGrid {
    let mut grid = SurfacePlanImageGrid::default();
    if visible.empty()
        || bounds.empty()
        || !visible.intersects(*bounds)
        || !(units_per_pixel > 0.0)
        || !units_per_pixel.is_finite()
    {
        return grid;
    }
    let meet = Box2::new(
        Point2::new(
            cpp::max(visible.min.x, bounds.min.x),
            cpp::max(visible.min.y, bounds.min.y),
        ),
        Point2::new(
            cpp::min(visible.max.x, bounds.max.x),
            cpp::min(visible.max.y, bounds.max.y),
        ),
    );
    let mut size = units_per_pixel;
    for _attempt in 0..4 {
        let c0 = ((meet.min.x - visible.min.x) / size).floor();
        let c1 = ((meet.max.x - visible.min.x) / size).ceil();
        let r0 = ((visible.max.y - meet.max.y) / size).floor();
        let r1 = ((visible.max.y - meet.min.y) / size).ceil();
        let width = cpp::max(1.0, c1 - c0);
        let height = cpp::max(1.0, r1 - r0);
        let pixels = width * height;
        let cap = std::cmp::max(pixel_cap, 1) as f64;
        if pixels > cap {
            // Coarser evenly, a hair past the ratio so the rounding outward
            // above cannot leave it just over.
            size *= (pixels / cap).sqrt() * 1.0001;
            continue;
        }
        grid.width = width as i32;
        grid.height = height as i32;
        grid.r#box = Box2::new(
            Point2::new(
                visible.min.x + c0 * size,
                visible.max.y - (r0 + height) * size,
            ),
            Point2::new(
                visible.min.x + (c0 + width) * size,
                visible.max.y - r0 * size,
            ),
        );
        return grid;
    }
    grid
}

// ---- why a style could not be drawn ----------------------------------------------------------

/// Why `contours` cannot be traced on `surface`, or empty when they can: an
/// interval that is not a finite number above zero, or one so fine that the
/// levels would cross the triangles more than PLAN_MAXIMUM_CONTOUR_POINTS
/// times. The refusal a paint makes before tracing (SurfacePlan::problem) and
/// the one a preflight check reports before a plot are this one answer. What
/// only tracing can find - a failure inside the tracer - is not predicted.
pub fn contour_refusal(surface: &TinSurface, contours: &SurfaceContours) -> String {
    if !(contours.interval > tolerance::GEOMETRIC)
        || !contours.interval.is_finite()
        || !contours.base.is_finite()
    {
        return String::from("contours: the interval must be a finite number greater than zero");
    }
    if estimate_contour_points(surface, contours.interval, contours.base)
        > PLAN_MAXIMUM_CONTOUR_POINTS
    {
        return format!(
            "contours: an interval of {} crosses this surface's triangles more than {} times; not drawn - set a larger interval",
            format_legend_value(contours.interval, 6),
            PLAN_MAXIMUM_CONTOUR_POINTS
        );
    }
    String::new()
}

/// The problems the styles of `surfaces` have, one for each visible surface
/// that has triangles and whose style shows contours it cannot trace, in the
/// order the surfaces were added: what a paint of the whole drawing would
/// report, found without painting. A hidden or empty surface is not drawn and
/// has none.
pub fn surface_style_problems(
    surfaces: &SurfaceStore,
    styles: &SurfaceStyleSet,
) -> Vec<SurfaceStyleProblem> {
    let mut found = Vec::new();
    for named in surfaces.all() {
        // The painter's own conditions for drawing a surface at all.
        let Some(surface) = &named.surface else {
            continue;
        };
        if !named.visible || surface.empty() {
            continue;
        }
        let style = style_of(styles, named);
        if !style.contours.show {
            continue;
        }
        let reason = contour_refusal(surface, &style.contours);
        if !reason.is_empty() {
            found.push(SurfaceStyleProblem {
                problem: SurfaceProblem {
                    surface: named.name.clone(),
                    style: style.name.clone(),
                    reason,
                },
                bounds: *surface.bounds(),
            });
        }
    }
    found
}

// ---- the plan --------------------------------------------------------------------------------

/// The colouring the plan view gives `surface`'s faces in `style` (its
/// plan_faces), with the range the surface spans where the style gives none.
/// What surface_plan colours the image by and what its legend says.
pub fn plan_colouring(surface: &TinSurface, style: &SurfaceDisplayStyle) -> SurfaceColouring {
    surface_colouring_for(style, style.plan_faces, surface)
}

/// `surface` in `style` through `view`. A surface with no triangles, or none
/// in view, draws nothing; whether the surface is visible is the caller's
/// question (NamedSurface::visible).
pub fn surface_plan(
    named: &NamedSurface,
    style: &SurfaceDisplayStyle,
    view: &SurfacePlanView<'_>,
    cache: &mut SurfacePlanCache,
) -> SurfacePlan {
    let mut plan = SurfacePlan {
        name: named.name.clone(),
        triangle_colour: style.triangle_colour,
        minor_colour: style.contours.minor_colour,
        major_colour: style.contours.major_colour,
        breakline_colour: style.breakline_colour,
        boundary_colour: style.boundary_colour,
        point_colour: style.point_colour,
        ..SurfacePlan::default()
    };
    let Some(shared) = &named.surface else {
        return plan;
    };
    if shared.empty()
        || view.visible.empty()
        || !(view.units_per_pixel > 0.0)
        || !view.units_per_pixel.is_finite()
    {
        return plan;
    }
    let index = cache.entry_for(shared);
    let surface: &TinSurface = shared;
    let pixel = view.units_per_pixel;
    let entry = &mut cache.entries[index];

    // The colouring, with the range the surface spans where the style gives
    // none - the steepest face from the cache, not a pass a paint.
    let colouring = if style.plan_faces == SurfaceFaces::Slope {
        surface_colouring(style, SurfaceFaces::Slope, 0.0, entry.steepest_percent)
    } else {
        surface_colouring(
            style,
            style.plan_faces,
            surface.min_elevation(),
            surface.max_elevation(),
        )
    };
    if colouring.colours_faces() {
        plan.legend = Some(surface_legend(&colouring));
    }

    // Culled to the view grown by a few pixels: a line's width, a mark.
    let reach = view.visible.inflated(4.0 * pixel);
    let mut marks = Vec::new();
    let triangles = triangles_in_box(entry, &reach, &mut marks);
    plan.stats.triangles_in_view = triangles.len();

    // ---- triangles: stroked, or too dense and tinted
    let mut dense = vec![false; triangles.len()];
    if style.show_triangles {
        let vertices = surface.vertices();
        let shortest = PLAN_EDGE_PIXELS * pixel;
        for (i, &t) in triangles.iter().enumerate() {
            let corners = surface.triangles()[t as usize];
            let mut any_stroked = false;
            for e in 0..3 {
                let a = vertices[corners[e] as usize];
                let b = vertices[corners[(e + 1) % 3] as usize];
                let edge = Segment2::new(Point2::new(a.x, a.y), Point2::new(b.x, b.y));
                if !(edge.length() >= shortest) {
                    continue;
                }
                any_stroked = true;
                // Once: from the lower-numbered of the two triangles, unless
                // the other is not being drawn.
                let across = surface.neighbors()[t as usize][e];
                if across != NO_TRIANGLE && across < t && marked(&marks, across) {
                    continue;
                }
                let mut bounds = Box2::default();
                bounds.expand(edge.start);
                bounds.expand(edge.end);
                if bounds.intersects(reach) {
                    plan.triangles.push(edge);
                }
            }
            dense[i] = !any_stroked;
            plan.stats.triangles_tinted += usize::from(dense[i]);
        }
        plan.stats.edges_stroked = plan.triangles.len();
    }

    // ---- points: marks, or pixels when too dense to tell apart
    let dense_points = style.show_points && entry.vertex_spacing < PLAN_POINT_PIXELS * pixel;
    let mut point_pixels = Vec::new();
    if style.show_points {
        let mut seen = vec![false; surface.vertex_count()];
        let vertices = surface.vertices();
        for &t in &triangles {
            for v in surface.triangles()[t as usize] {
                if seen[v as usize] {
                    continue;
                }
                seen[v as usize] = true;
                let at = Point2::new(vertices[v as usize].x, vertices[v as usize].y);
                if !reach.contains(at) {
                    continue;
                }
                if dense_points {
                    point_pixels.push(at);
                } else {
                    plan.points.push(at);
                }
            }
        }
        plan.stats.points_marked = plan.points.len();
    }

    // ---- the image: faces, dense triangles, dense points
    let tint = style.show_triangles && plan.stats.triangles_tinted > 0;
    let want_image = colouring.colours_faces() || tint || !point_pixels.is_empty();
    if want_image && view.image_width > 0 && view.image_height > 0 && !view.image_box.empty() {
        plan.image.r#box = view.image_box;
        plan.image.width = view.image_width;
        plan.image.height = view.image_height;
        plan.image.pixels = vec![0; view.image_width as usize * view.image_height as usize];
        plan.stats.image_pixels = plan.image.pixels.len();
        let rule = FaceRule {
            faces: colouring.faces,
            colouring: Some(&colouring),
            opacity: style.plan_opacity,
            tint,
            tint_colour: premultiplied(style.triangle_colour, PLAN_DENSE_TRIANGLE_ALPHA),
        };
        let image = FaceImage::new(surface, view);
        let pool = match view.pool {
            Some(pool) => pool,
            None => TaskPool::shared(),
        };
        plan.stats.pixels_covered =
            image.paint(&mut plan.image.pixels, &triangles, &dense, &rule, pool);
        let point_colour = premultiplied(style.point_colour, 1.0);
        for &at in &point_pixels {
            plan.stats.points_in_image +=
                usize::from(image.dot(&mut plan.image.pixels, at, point_colour));
        }
    }

    let mut scratch = PolylineRuns::default();
    let mut closed_scratch = Vec::new();
    let thin = PLAN_SIMPLIFY_PIXELS * pixel;

    // ---- contours, traced once per surface and settings
    if style.contours.show {
        let settings = &style.contours;
        let stale = entry.contours.as_ref().is_none_or(|kept| {
            kept.interval != settings.interval
                || kept.base != settings.base
                || kept.major_every != settings.major_every
        });
        if stale {
            entry.contours = Some(trace_contours(surface, settings));
            cache.tracings += 1;
        }
        let entry = &cache.entries[index];
        let Some(set) = entry.contours.as_ref() else {
            unreachable!("the contours were traced above");
        };
        plan.problem = set.problem.clone();
        plan.stats.contour_levels = set.levels;
        // How far apart a kind of contour lies on screen, at the surface's
        // mean slope: interval over slope is the mean spacing in the model.
        let spacing_pixels = |interval: f64| {
            if entry.mean_slope > 0.0 {
                interval / entry.mean_slope / pixel
            } else {
                f64::INFINITY
            }
        };
        plan.stats.minor_contours_hidden = spacing_pixels(settings.interval) < PLAN_CONTOUR_PIXELS;
        plan.stats.major_contours_hidden = settings.major_every <= 0
            || spacing_pixels(settings.interval * f64::from(settings.major_every))
                < PLAN_CONTOUR_PIXELS;
        // Minors first, then majors, so a major is drawn over the minors.
        for major in [false, true] {
            let hidden = if major {
                plan.stats.major_contours_hidden
            } else {
                plan.stats.minor_contours_hidden
            };
            if hidden {
                continue;
            }
            for line in &set.lines {
                if line.major != major {
                    continue;
                }
                // Too small to see: a speck.
                if cpp::max(line.bounds.width(), line.bounds.height()) < pixel {
                    continue;
                }
                visible_runs(
                    line,
                    &reach,
                    thin,
                    &mut scratch,
                    &mut closed_scratch,
                    |run| {
                        plan.stats.contour_vertices += run.len();
                        plan.contours.push(SurfacePlanContour {
                            level: line.level,
                            major: line.major,
                            points: run,
                        });
                    },
                );
            }
        }
        plan.stats.contour_runs = plan.contours.len();

        // Labels on the majors drawn, along each whole contour from its own
        // start - so a pan does not slide them along the line - inside the
        // view by a margin.
        if settings.label_major && !plan.stats.major_contours_hidden {
            let decimals = legend_decimals(&[settings.interval, settings.base]);
            let spacing = cpp::max(view.label_spacing_pixels, 1.0) * pixel;
            let inside = view.visible.inflated(-PLAN_LABEL_MARGIN_PIXELS * pixel);
            for line in &set.lines {
                if !line.major
                    || line.points.len() < 2
                    || line.length < PLAN_LABEL_MINIMUM_PIXELS * pixel
                    || !line.bounds.intersects(inside)
                    || plan.labels.len() >= PLAN_MAXIMUM_LABELS
                {
                    continue;
                }
                let text = format_legend_value(line.level, decimals);
                let mut next = cpp::min(0.5 * spacing, 0.5 * line.length);
                let mut walked = 0.0;
                let segments = if line.closed {
                    line.points.len()
                } else {
                    line.points.len() - 1
                };
                let mut s = 0;
                while s < segments && plan.labels.len() < PLAN_MAXIMUM_LABELS {
                    let a = line.points[s];
                    let b = line.points[(s + 1) % line.points.len()];
                    let length = a.distance_to(b);
                    while length > 0.0
                        && next <= walked + length
                        && plan.labels.len() < PLAN_MAXIMUM_LABELS
                    {
                        let f = (next - walked) / length;
                        let at = Point2::new(a.x + (b.x - a.x) * f, a.y + (b.y - a.y) * f);
                        if inside.contains(at) {
                            plan.labels.push(SurfacePlanLabel {
                                at,
                                angle: (b.y - a.y).atan2(b.x - a.x),
                                text: text.clone(),
                            });
                        }
                        next += spacing;
                    }
                    walked += length;
                    s += 1;
                }
            }
            plan.stats.labels = plan.labels.len();
        }
    }

    // ---- breaklines and the boundary, joined once
    if style.show_breaklines || style.show_boundary {
        let entry = &mut cache.entries[index];
        if !entry.lines_built {
            build_lines(entry);
        }
        if style.show_breaklines {
            for line in &entry.breaklines {
                visible_runs(
                    line,
                    &reach,
                    thin,
                    &mut scratch,
                    &mut closed_scratch,
                    |run| plan.breaklines.push(run),
                );
            }
            plan.stats.breakline_runs = plan.breaklines.len();
        }
        if style.show_boundary {
            for line in &entry.rim {
                visible_runs(
                    line,
                    &reach,
                    thin,
                    &mut scratch,
                    &mut closed_scratch,
                    |run| plan.boundary.push(run),
                );
            }
            plan.stats.boundary_runs = plan.boundary.len();
        }
    }
    plan
}
