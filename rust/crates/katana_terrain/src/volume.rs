//! Volumes and surface comparison (docs/terrain.md; docs/volumes.md for the
//! methods, their sources and the verbs that offer them).
//!
//! The exact methods integrate a function that is linear over every triangle,
//! so each triangle is integrated in closed form; a triangle on which the
//! function changes sign is split along its zero line and the two parts are
//! accumulated separately. Nothing is sampled: the results are exact up to
//! floating-point rounding (sums are compensated). Two surfaces are compared
//! on their overlay (overlay), whose every piece lies inside one triangle of
//! each, so their difference is linear on it.
//!
//! The grid method (grid_volume) is the one sampled method, kept as an
//! independent check of the exact ones and because it is what a grid of
//! cells (a DEM, a borrow pit) is measured by.
//!
//! The C++ overloads that take a region have names of their own here:
//! `volume_to_datum_inside` and `compare_surfaces_inside`.

use std::sync::Mutex;

use katana_core::error::make_error;
use katana_core::task_pool::TaskPool;
use katana_core::text::format_exact_real;
use katana_core::{Error, ErrorCode, Result};
use katana_geometry::Polyline2;
use katana_math::cpp;
use katana_math::numerics::tolerance;

use crate::overlay::{
    OverlayPiece, OverlayRegion, OverlayVisitor, overlay_block_count, visit_overlay,
};
use crate::section_volume::SegmentProfiler;
use crate::summation::CompensatedSum;
use crate::tin_builder::{DuplicatePointPolicy, TinBuildOptions, TinInput, build_tin};
use crate::tin_surface::{Box2, Point2, Point3, TinSurface, compute_by_index};

/// Volume between a surface and the horizontal plane z = datum. Cubic metres
/// and square metres for metre coordinates.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DatumVolume {
    /// solid between datum and surface where the surface is higher (>= 0)
    pub above: f64,
    /// solid between surface and datum where the surface is lower (>= 0)
    pub below: f64,
    /// above - below = integral of (z - datum) over the plan area
    pub net: f64,
    pub plan_area_above: f64,
    pub plan_area_below: f64,
    /// Earthwork reading for a level design at `datum`: cut = above, fill =
    /// below. The plan area measured: the surface's, or the part of it inside
    /// a region. plan_area_above + plan_area_below, less where the surface is
    /// exactly at the datum.
    pub plan_area: f64,
}

/// Earthwork between an existing and a design surface over their common plan
/// area.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SurfaceComparison {
    /// where existing is above design: material to remove (>= 0)
    pub cut: f64,
    /// where design is above existing: material to place (>= 0)
    pub fill: f64,
    /// fill - cut = integral of (zDesign - zExisting)
    pub net: f64,
    /// common area that was compared
    pub plan_area: f64,
    pub cut_area: f64,
    /// plan_area - cut_area - fill_area is where the surfaces coincide: every
    /// corner of a piece within GEOMETRIC of the other surface's height is on
    /// it, so rounding (+/- 1e-15 where two readings of one plane differ in
    /// the last bit) is not booked as cut or fill. The volumes are not
    /// rounded.
    pub fill_area: f64,
    /// triangles of the overlay inside the common area
    pub overlay_triangle_count: usize,
}

/// The plan area of a piece on which a function is linear, split into where it
/// is above zero and where it is below, by its values at the corners.
#[derive(Clone, Copy, Debug, Default)]
struct AreaSplit {
    positive: f64,
    negative: f64,
}

/// The lone vertex of a triangle whose values differ in sign: the only
/// positive one, or the only negative one.
fn lone_vertex(f: &[f64; 3], lone_positive: bool) -> usize {
    let mut p = 0;
    for (i, &value) in f.iter().enumerate() {
        let lone = if lone_positive {
            value > 0.0
        } else {
            value < 0.0
        };
        if lone {
            p = i;
        }
    }
    p
}

/// With vertex values f0, f1, f2 and plan area A, when the signs differ exactly
/// one vertex p stands alone against q and r. The zero line meets edges pq and
/// pr at the fractions tq = fp/(fp-fq) and tr = fp/(fp-fr), cutting off the
/// triangle (p, mq, mr) of area A*tq*tr; the rest of the piece is on the other
/// side. A corner at zero belongs to neither side.
fn split_area(area: f64, f: &[f64; 3]) -> AreaSplit {
    let positives = f.iter().filter(|&&value| value > 0.0).count();
    let negatives = f.iter().filter(|&&value| value < 0.0).count();
    if negatives == 0 || positives == 0 {
        return AreaSplit {
            positive: if positives != 0 { area } else { 0.0 },
            negative: if negatives != 0 { area } else { 0.0 },
        };
    }
    let lone_positive = positives == 1;
    let p = lone_vertex(f, lone_positive);
    let fp = f[p].abs();
    let fq = f[(p + 1) % 3].abs();
    let fr = f[(p + 2) % 3].abs();
    let tip_area = area * (fp / (fp + fq)) * (fp / (fp + fr));
    if lone_positive {
        AreaSplit {
            positive: tip_area,
            negative: area - tip_area,
        }
    } else {
        AreaSplit {
            positive: area - tip_area,
            negative: tip_area,
        }
    }
}

/// Integral of a function that is linear over a triangle, positive and
/// negative part kept apart.
///
/// With vertex values f0, f1, f2 and plan area A the integral is
/// A * (f0+f1+f2)/3. When the signs differ, exactly one vertex p stands alone
/// against q and r. The zero line meets edges pq and pr at the fractions
/// tq = fp/(fp-fq) and tr = fp/(fp-fr), cutting off the triangle (p, mq, mr)
/// of area A*tq*tr with values (fp, 0, 0). The rest is the quadrilateral
/// (mq, q, r, mr), split into the triangles (mq, q, r) of area A*(1-tq) and
/// (mq, r, mr) of area A*tq*(1-tr). Every term of a part has the same sign, so
/// nothing cancels, and no coordinates are constructed.
///
/// THE AREAS ARE KEPT BY WHAT THE SURFACES DO, NOT BY THE LAST BIT. The volumes
/// are summed from f as it is, so their bits are what they always were. The
/// plan areas where the function is above and below zero are sorted by f with
/// every corner within GEOMETRIC (0.1 micrometre) of zero taken as zero: two
/// coincident surfaces are read at a piece's corners by barycentric weights
/// that sum to 1 only to a few ulps, so their difference there is +/- 1e-15
/// and not 0, and a sort by the sign of the last bit booked whole pieces of
/// ground that does not change as cut area or fill area (a flat surface
/// against its own level inside a skewed polygon: 44.6 m2 of cut and 23.3 of
/// fill; a stockpile on planar ground: 700 m2 of cut round a toe of 400). A
/// height difference under 0.1 micrometre is no earthwork on any survey, and
/// the volume it holds, under that times the area, stays in the volumes.
#[derive(Clone, Copy, Debug, Default)]
struct SignedIntegral {
    positive: CompensatedSum,
    negative: CompensatedSum,
    positive_area: CompensatedSum,
    negative_area: CompensatedSum,
}

impl SignedIntegral {
    fn add(&mut self, area: f64, f: [f64; 3]) {
        if !(area > 0.0) {
            return;
        }
        let positives = f.iter().filter(|&&value| value > 0.0).count();
        let negatives = f.iter().filter(|&&value| value < 0.0).count();
        if negatives == 0 || positives == 0 {
            let volume = area * ((f[0] + f[1] + f[2]) / 3.0);
            if positives != 0 {
                self.positive.add(volume);
            } else if negatives != 0 {
                self.negative.add(-volume);
            }
        } else {
            // Mixed signs: the lone vertex is the only positive or the only
            // negative one.
            let lone_positive = positives == 1;
            let p = lone_vertex(&f, lone_positive);
            let fp = f[p].abs();
            // the others lie on the opposite side or on the zero line
            let fq = f[(p + 1) % 3].abs();
            let fr = f[(p + 2) % 3].abs();
            let tq = fp / (fp + fq);
            let tr = fp / (fp + fr);
            let tip_area = area * tq * tr;
            let tip_volume = tip_area * fp / 3.0;
            let rest_volume =
                area * (1.0 - tq) * (fq + fr) / 3.0 + area * tq * (1.0 - tr) * fr / 3.0;
            if lone_positive {
                self.positive.add(tip_volume);
                self.negative.add(rest_volume);
            } else {
                self.negative.add(tip_volume);
                self.positive.add(rest_volume);
            }
        }

        let mut counted = f;
        for value in &mut counted {
            if value.abs() <= tolerance::GEOMETRIC {
                *value = 0.0;
            }
        }
        let areas = split_area(area, &counted);
        if areas.positive != 0.0 {
            self.positive_area.add(areas.positive);
        }
        if areas.negative != 0.0 {
            self.negative_area.add(areas.negative);
        }
    }

    /// Folds another integral's four sums into this one's. Used to combine the
    /// per-block integrals of compare_surfaces in block order; each block's sum
    /// arrives as its compensated value, so the combination is itself a
    /// compensated sum of a fixed sequence of terms.
    fn merge(&mut self, other: &SignedIntegral) {
        self.positive.add(other.positive());
        self.negative.add(other.negative());
        self.positive_area.add(other.positive_area());
        self.negative_area.add(other.negative_area());
    }

    fn positive(&self) -> f64 {
        self.positive.value()
    }
    /// as a magnitude
    fn negative(&self) -> f64 {
        self.negative.value()
    }
    fn positive_area(&self) -> f64 {
        self.positive_area.value()
    }
    fn negative_area(&self) -> f64 {
        self.negative_area.value()
    }
}

/// One block's sums. One cache line each: neighbouring blocks are written by
/// different threads, and sharing a line would have them invalidate each
/// other.
#[derive(Clone, Copy, Debug, Default)]
#[repr(align(64))]
struct BlockSums {
    integral: SignedIntegral,
    plan_area: CompensatedSum,
    pieces: usize,
}

/// The overlay's pieces summed one block at a time, each block into its own
/// slot, and the slots combined in block order: the same sequence of
/// floating-point operations whatever the thread count (overlay).
struct ComparisonSums {
    blocks: Vec<Mutex<BlockSums>>,
}

impl ComparisonSums {
    fn new(blocks: usize) -> Self {
        Self {
            blocks: (0..blocks)
                .map(|_| Mutex::new(BlockSums::default()))
                .collect(),
        }
    }

    fn result(self) -> SurfaceComparison {
        let mut integral = SignedIntegral::default();
        let mut plan_area = CompensatedSum::new();
        let mut comparison = SurfaceComparison::default();
        for block in self.blocks {
            let sums = block
                .into_inner()
                .unwrap_or_else(|error| error.into_inner());
            integral.merge(&sums.integral);
            plan_area.add(sums.plan_area.value());
            comparison.overlay_triangle_count += sums.pieces;
        }
        comparison.fill = integral.positive();
        comparison.cut = integral.negative();
        comparison.net = comparison.fill - comparison.cut;
        comparison.plan_area = plan_area.value();
        comparison.fill_area = integral.positive_area();
        comparison.cut_area = integral.negative_area();
        comparison
    }
}

impl OverlayVisitor for ComparisonSums {
    fn piece(&self, block: usize, piece: &OverlayPiece) {
        // One block's pieces arrive on one thread, so the lock is never
        // contended.
        let mut sums = self.blocks[block]
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        sums.integral.add(
            piece.area,
            [
                piece.z_top[0] - piece.z_base[0],
                piece.z_top[1] - piece.z_base[1],
                piece.z_top[2] - piece.z_base[2],
            ],
        );
        sums.plan_area.add(piece.area);
        sums.pieces += 1;
    }
}

fn compare(
    existing: &TinSurface,
    design: &TinSurface,
    region: Option<&OverlayRegion>,
    pool: Option<&TaskPool>,
) -> Result<SurfaceComparison> {
    let sums = ComparisonSums::new(overlay_block_count(existing));
    visit_overlay(existing, design, &sums, region, pool)?;
    Ok(sums.result())
}

/// A comparison read as a volume to a level: the surface is the existing one,
/// the level the design, so what stands above the level is the cut.
fn as_datum_volume(comparison: &SurfaceComparison) -> DatumVolume {
    DatumVolume {
        above: comparison.cut,
        below: comparison.fill,
        net: comparison.cut - comparison.fill,
        plan_area_above: comparison.cut_area,
        plan_area_below: comparison.fill_area,
        plan_area: comparison.plan_area,
    }
}

/// Fails with InvalidArgument for a non-finite datum or an empty surface.
pub fn volume_to_datum(surface: &TinSurface, datum: f64) -> Result<DatumVolume> {
    if !datum.is_finite() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "datum elevation must be finite",
        ));
    }
    if surface.empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "surface has no triangles",
        ));
    }
    let mut integral = SignedIntegral::default();
    let mut plan_area = CompensatedSum::new();
    for t in 0..surface.triangle_count() {
        let tri = surface.triangles()[t];
        let vertices = surface.vertices();
        // Heights above the datum first: subtracting the datum from the summed
        // prism volume instead would cancel most digits for elevations far
        // from 0.
        let area = surface.triangle_plan_area(t);
        integral.add(
            area,
            [
                vertices[tri[0] as usize].z - datum,
                vertices[tri[1] as usize].z - datum,
                vertices[tri[2] as usize].z - datum,
            ],
        );
        plan_area.add(area);
    }
    Ok(DatumVolume {
        above: integral.positive(),
        below: integral.negative(),
        net: integral.positive() - integral.negative(),
        plan_area_above: integral.positive_area(),
        plan_area_below: integral.negative_area(),
        plan_area: plan_area.value(),
    })
}

/// The same over the part of the surface inside `region` (the C++ overload of
/// volumeToDatum with a region): the surface compared with the region made
/// flat at the datum (OverlayRegion::at_level), so the region's edges cut the
/// surface's triangles exactly. A region partly off the surface measures the
/// part on it, which plan_area says.
pub fn volume_to_datum_inside(
    surface: &TinSurface,
    datum: f64,
    region: &OverlayRegion,
    pool: Option<&TaskPool>,
) -> Result<DatumVolume> {
    if !datum.is_finite() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "datum elevation must be finite",
        ));
    }
    if surface.empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "surface has no triangles",
        ));
    }
    if region.empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the region has no area",
        ));
    }
    let level = region.at_level(datum)?;
    let comparison = compare(surface, &level, None, pool)?;
    Ok(as_datum_volume(&comparison))
}

/// Exact, on the overlay of the two (overlay): each triangle of the existing
/// surface is clipped against the design triangles it meets, and each piece
/// (inside one triangle of each, so zDesign - zExisting is linear over it) is
/// integrated in closed form. Surfaces that do not overlap give a result with
/// plan_area == 0. Fails with InvalidArgument for an empty surface.
///
/// Runs on `pool` (None: TaskPool::shared()). The overlay's pieces arrive in
/// blocks of a fixed number of existing triangles; each block is summed in its
/// fixed order and the block sums are combined in block order, so the result
/// is the same bits at any thread count, TaskPool::new(0) included (CLAUDE.md
/// section 5). Safe to call from a background job: both surfaces are only
/// read.
pub fn compare_surfaces(
    existing: &TinSurface,
    design: &TinSurface,
    pool: Option<&TaskPool>,
) -> Result<SurfaceComparison> {
    compare(existing, design, None, pool)
}

/// The same over the part of the common area inside `region` (the C++
/// overload of compareSurfaces with a region): the overlay cut again by the
/// region's triangles, so the polygon's edges are followed exactly, not by
/// whole triangles.
pub fn compare_surfaces_inside(
    existing: &TinSurface,
    design: &TinSurface,
    region: &OverlayRegion,
    pool: Option<&TaskPool>,
) -> Result<SurfaceComparison> {
    if region.empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the region has no area",
        ));
    }
    compare(existing, design, Some(region), pool)
}

// ---- stockpiles and pits ------------------------------------------------------------------

/// What a stockpile (or a pit) is measured against: a base made from its
/// boundary.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StockpileBase {
    /// The boundary's heights, triangulated inside it: the toe of the pile as
    /// it stands. The boundary's own heights when every vertex has one,
    /// otherwise the surface's at each vertex.
    #[default]
    Boundary,
    /// A level plane at the lowest point of the surface along the boundary:
    /// its vertices and every place an edge of the surface crosses it.
    Lowest,
    /// A level plane at a given height.
    Level,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct StockpileOptions {
    pub base: StockpileBase,
    /// StockpileBase::Level
    pub level: f64,
    /// StockpileBase::Boundary: the boundary's own height at each of its
    /// vertices, when it has one (a 3D string surveyed along the toe). Used
    /// only when every vertex has one; otherwise ignored for the surface's.
    pub boundary_heights: Vec<Option<f64>>,
    /// Rings inside the boundary that are not part of the pile (a tagged hole
    /// of the toe's polygon): nothing inside them is measured, their edges are
    /// boundaries like the outer ring's for the lowest point, and a Boundary
    /// base is made round them (at the surface's heights, a hole having no
    /// heights of its own) and not across them. Any orientation; the first
    /// vertex may be repeated at the end.
    pub holes: Vec<Polyline2>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StockpileVolume {
    /// above: the pile, the surface above its base (what is cut to remove
    /// it); below: a pit, the surface below its base (what is filled).
    pub volume: DatumVolume,
    pub base: StockpileBase,
    /// Lowest and Level: the height of the base.
    pub level: Option<f64>,
    /// Boundary: whether the heights were the boundary's own.
    pub boundary_heights: bool,
    /// The boundary's plan area; volume.plan_area is the part of it on the
    /// surface.
    pub boundary_area: f64,
}

/// Fails with InvalidArgument for an empty surface or a non-finite level;
/// InvalidGeometry for a boundary that is no polygon, or holes that do not lie
/// in it (OverlayRegion::from_polygon), a Boundary base whose boundary has a
/// vertex off the surface and no height of its own, or a hole with a vertex off
/// the surface (naming the vertex: absent is not zero), and a Lowest base whose
/// boundary never crosses the surface.
pub fn stockpile_volume(
    surface: &TinSurface,
    boundary: &Polyline2,
    options: &StockpileOptions,
    pool: Option<&TaskPool>,
) -> Result<StockpileVolume> {
    if surface.empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "surface has no triangles",
        ));
    }
    if options.base == StockpileBase::Level && !options.level.is_finite() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the base level must be finite",
        ));
    }
    // A ring drawn back to its start repeats its first vertex at its end: the
    // same ring without it, and its height list with it.
    let mut ring = boundary.clone();
    ring.closed = true;
    let mut ring_heights = options.boundary_heights.clone();
    while ring.vertices.len() > 1
        && ring.vertices[0].distance_to(ring.vertices[ring.vertices.len() - 1])
            <= tolerance::GEOMETRIC
    {
        ring.vertices.pop();
        if ring_heights.len() > ring.vertices.len() {
            ring_heights.pop();
        }
    }
    let region = OverlayRegion::from_polygon(&ring, &options.holes)?;

    let mut result = StockpileVolume {
        base: options.base,
        boundary_area: region.area(),
        ..StockpileVolume::default()
    };
    let base = if options.base == StockpileBase::Boundary {
        // The toe as it stands: the boundary's own heights when EVERY vertex
        // has one, otherwise the ground's at each vertex (a vertex's own
        // height is then not used, even where the ground has none under it:
        // mixing the two would build a base from two sources). A vertex off
        // the ground with no height to use is refused: its height would be a
        // guess, and a base at 0 there would add a cone of nothing.
        let has_own = |index: usize| {
            index < ring_heights.len() && ring_heights[index].is_some_and(f64::is_finite)
        };
        let own = !ring_heights.is_empty()
            && ring_heights.len() == ring.vertices.len()
            && ring_heights
                .iter()
                .all(|height| height.is_some_and(f64::is_finite));
        result.boundary_heights = own;
        let mut input = TinInput::default();
        for (v, &at) in ring.vertices.iter().enumerate() {
            let height = if own {
                ring_heights[v]
            } else {
                surface.elevation_at(at)
            };
            let Some(height) = height else {
                if has_own(v) {
                    // This vertex has a height, but the rule is all or none:
                    // say which vertex is the one that has none.
                    let mut missing = 0;
                    while missing < ring.vertices.len() && has_own(missing) {
                        missing += 1;
                    }
                    return Err(make_error(
                        ErrorCode::InvalidGeometry,
                        format!(
                            "boundary vertex {} is off the surface, and its own height is not \
                             used: the boundary's own heights are used only when every vertex \
                             has one, and vertex {} has none; give every vertex a height, or use \
                             the lowest point or a level",
                            v + 1,
                            missing + 1
                        ),
                        format!("vertex={} missing={}", v + 1, missing + 1),
                    ));
                }
                return Err(make_error(
                    ErrorCode::InvalidGeometry,
                    format!(
                        "boundary vertex {} is off the surface and has no height of its own, so \
                         the base has no height there; give the boundary heights, or use the \
                         lowest point or a level",
                        v + 1
                    ),
                    format!("vertex={}", v + 1),
                ));
            };
            input.points.push(Point3::new(at.x, at.y, height));
        }
        // A hole is not part of the pile and has no heights of its own: its
        // edge stands on the ground, and a vertex of it off the ground has no
        // height to give the base (absent is not zero).
        let mut holes: Vec<Polyline2> = Vec::with_capacity(options.holes.len());
        for (h, given) in options.holes.iter().enumerate() {
            let mut hole = given.clone();
            hole.closed = true;
            while hole.vertices.len() > 1
                && hole.vertices[0].distance_to(hole.vertices[hole.vertices.len() - 1])
                    <= tolerance::GEOMETRIC
            {
                hole.vertices.pop();
            }
            for (v, &at) in hole.vertices.iter().enumerate() {
                let Some(height) = surface.elevation_at(at) else {
                    return Err(make_error(
                        ErrorCode::InvalidGeometry,
                        format!(
                            "vertex {} of hole {} is off the surface and has no height of its \
                             own, so the base has no height there; use the lowest point or a \
                             level",
                            v + 1,
                            h + 1
                        ),
                        format!("hole={} vertex={}", h + 1, v + 1),
                    ));
                };
                input.points.push(Point3::new(at.x, at.y, height));
            }
            holes.push(hole);
        }
        input.holes = holes;
        input.boundary = ring;
        // Two vertices of the ring at one place (a ring that doubles back to
        // touch itself is refused by the triangulator anyway) keep the first's
        // height.
        let build_options = TinBuildOptions {
            duplicate_points: DuplicatePointPolicy::KeepFirst,
            ..TinBuildOptions::default()
        };
        let built = build_tin(&input, &build_options).map_err(|error| {
            make_error(
                ErrorCode::InvalidGeometry,
                format!("the boundary cannot be made a base: {}", error.message),
                error.context,
            )
        })?;
        built.surface
    } else {
        let mut level = options.level;
        if options.base == StockpileBase::Lowest {
            // The lowest point along the boundary is at a vertex of it or
            // where an edge of the surface crosses it: the ends of the pieces
            // the profiler gives, which are exact.
            let profiler = SegmentProfiler::new(surface);
            let mut lowest: Option<f64> = None;
            let mut lower = |z: f64| {
                lowest = Some(match lowest {
                    Some(low) => cpp::min(low, z),
                    None => z,
                });
            };
            // The edge of a hole is a boundary of the pile as the outer ring
            // is.
            for edge in std::iter::once(&ring).chain(&options.holes) {
                let count = edge.vertices.len();
                for v in 0..count {
                    let from = edge.vertices[v];
                    let to = edge.vertices[(v + 1) % count];
                    for piece in profiler.along(from, to) {
                        lower(cpp::min(piece.z0, piece.z1));
                    }
                    if let Some(at) = surface.elevation_at(from) {
                        lower(at);
                    }
                }
            }
            let Some(lowest) = lowest else {
                return Err(Error::new(
                    ErrorCode::InvalidGeometry,
                    "the boundary does not cross the surface anywhere, so it has no lowest point \
                     on it",
                ));
            };
            level = lowest;
        }
        result.level = Some(level);
        region.at_level(level)?
    };

    let comparison = compare(surface, &base, None, pool)?;
    result.volume = as_datum_volume(&comparison);
    Ok(result)
}

// ---- the grid method -----------------------------------------------------------------------

/// A cross-check of the exact methods, and the way a grid of cells is
/// measured: square cells of side `cell`, on lines at whole multiples of the
/// cell from the origin (so grids of one cell size line up, as RASTER GRID's
/// do), each counted when its centre is on both surfaces (and inside the
/// region, when one is given) as a prism of the cell's area and the difference
/// at its centre (the midpoint rule). Converges on the exact answer as the
/// cell shrinks; on a surface whose ridges run through cells it is out by a
/// known amount (docs/volumes.md, "The grid method").
///
/// A centre on a polygon's edge is inside it (OverlayRegion::contains), so a
/// cell centred on an edge two polygons share belongs to both: lots measured
/// one at a time and summed would count that line of cells twice (+432 m3 of
/// cut on a site of four quadrants). Measuring several polygons, each is given
/// the polygons before it as `claimed`, and a cell any of them holds is left
/// to the first that did: the cells counted, summed over the polygons, are the
/// cells of their union, each once.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GridVolume {
    /// existing above design, as SurfaceComparison
    pub cut: f64,
    pub fill: f64,
    /// fill - cut
    pub net: f64,
    /// cells counted x cell area
    pub plan_area: f64,
    pub cells: usize,
    pub cell: f64,
}

/// The grid of cells is capped, as a surface's raster export caps its grid
/// (interop's terrain_io, maxCells): more cells than this is a unit mistake,
/// and the refusal names the cell that would fit.
pub const MAX_GRID_VOLUME_CELLS: u64 = 25_000_000;

/// One block of rows' sums.
#[derive(Clone, Copy, Debug, Default)]
struct GridBlock {
    cut: CompensatedSum,
    fill: CompensatedSum,
    cells: usize,
}

/// InvalidArgument for an empty surface, a cell that is not finite and
/// positive, or a grid of more than MAX_GRID_VOLUME_CELLS. Surfaces with no
/// ground in common count no cells. The same bits at any thread count: rows of
/// cells are summed in fixed blocks combined in order.
///
/// `claimed`: regions whose cells are already counted (the polygons measured
/// before this one). A cell whose centre any of them holds, the boundary
/// included, is not counted again. Empty: none.
pub fn grid_volume(
    existing: &TinSurface,
    design: &TinSurface,
    cell: f64,
    region: Option<&OverlayRegion>,
    pool: Option<&TaskPool>,
    claimed: &[OverlayRegion],
) -> Result<GridVolume> {
    if existing.empty() || design.empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "both surfaces must have triangles",
        ));
    }
    if !(cell > 0.0) || !cell.is_finite() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the cell must be a positive length",
        ));
    }
    let mut result = GridVolume {
        cell,
        ..GridVolume::default()
    };
    if !existing.bounds().intersects(*design.bounds()) {
        return Ok(result);
    }
    let mut common = Box2::new(
        Point2::new(
            cpp::max(existing.bounds().min.x, design.bounds().min.x),
            cpp::max(existing.bounds().min.y, design.bounds().min.y),
        ),
        Point2::new(
            cpp::min(existing.bounds().max.x, design.bounds().max.x),
            cpp::min(existing.bounds().max.y, design.bounds().max.y),
        ),
    );
    if let Some(region) = region {
        if region.empty() || !common.intersects(*region.bounds()) {
            return Ok(result);
        }
        common = Box2::new(
            Point2::new(
                cpp::max(common.min.x, region.bounds().min.x),
                cpp::max(common.min.y, region.bounds().min.y),
            ),
            Point2::new(
                cpp::min(common.max.x, region.bounds().max.x),
                cpp::min(common.max.y, region.bounds().max.y),
            ),
        );
    }
    // Every cell whose centre can be inside the common box: lines at whole
    // multiples of the cell, from the one at or below the box's lower edge.
    let first_column = (common.min.x / cell).floor();
    let first_row = (common.min.y / cell).floor();
    let columns_real = (common.max.x / cell).ceil() - first_column;
    let rows_real = (common.max.y / cell).ceil() - first_row;
    if !(columns_real * rows_real <= MAX_GRID_VOLUME_CELLS as f64) {
        let fits = (common.width() * common.height() / MAX_GRID_VOLUME_CELLS as f64).sqrt();
        // The C++ casts the counts to an unsigned integer unguarded (undefined
        // for one out of range); Rust's cast saturates.
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!(
                "a grid of {} x {} cells is more than the {MAX_GRID_VOLUME_CELLS} allowed; a \
                 cell of at least {} fits",
                columns_real as u64,
                rows_real as u64,
                format_exact_real((fits * 1000.0).ceil() / 1000.0)
            ),
        ));
    }
    let columns = cpp::max(columns_real, 0.0) as usize;
    let rows = cpp::max(rows_real, 0.0) as usize;
    let cell_area = cell * cell;

    // Rows in blocks of a fixed size, each summed in order into its own slot,
    // and the slots combined in order: the same bits at any thread count.
    const ROWS_PER_BLOCK: usize = 16;
    let block_count = rows.div_ceil(ROWS_PER_BLOCK);
    let workers: &TaskPool = match pool {
        Some(pool) => pool,
        None => TaskPool::shared(),
    };
    let blocks = compute_by_index(workers, block_count, 1, GridBlock::default(), |block| {
        let mut sums = GridBlock::default();
        let row_end = ((block + 1) * ROWS_PER_BLOCK).min(rows);
        for row in block * ROWS_PER_BLOCK..row_end {
            let y = (first_row + row as f64 + 0.5) * cell;
            for column in 0..columns {
                let centre = Point2::new((first_column + column as f64 + 0.5) * cell, y);
                if !common.contains(centre) {
                    continue;
                }
                if region.is_some_and(|region| !region.contains(centre)) {
                    continue;
                }
                // Counted with an earlier polygon already: a centre on an edge
                // two polygons share is inside both.
                if claimed.iter().any(|earlier| {
                    earlier
                        .bounds()
                        .inflated(tolerance::GEOMETRIC)
                        .contains(centre)
                        && earlier.contains(centre)
                }) {
                    continue;
                }
                let Some(z_existing) = existing.elevation_at(centre) else {
                    continue;
                };
                let Some(z_design) = design.elevation_at(centre) else {
                    continue;
                };
                let depth = z_design - z_existing;
                if depth > 0.0 {
                    sums.fill.add(depth * cell_area);
                } else if depth < 0.0 {
                    sums.cut.add(-depth * cell_area);
                }
                sums.cells += 1;
            }
        }
        sums
    });

    let mut cut = CompensatedSum::new();
    let mut fill = CompensatedSum::new();
    for sums in &blocks {
        cut.add(sums.cut.value());
        fill.add(sums.fill.value());
        result.cells += sums.cells;
    }
    result.cut = cut.value();
    result.fill = fill.value();
    result.net = result.fill - result.cut;
    result.plan_area = result.cells as f64 * cell_area;
    Ok(result)
}
