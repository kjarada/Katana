//! Volumes from cross sections along an alignment (docs/volumes.md, "Along an
//! alignment").
//!
//! The earthwork between two surfaces measured the way a road job is measured
//! and paid: cross sections cut square to the alignment at an interval and at
//! its key stations, the area between the two surfaces on each, and the volume
//! between consecutive sections from their areas.
//!
//! THE AREAS ARE EXACT. A section is a straight plan segment; each surface
//! along it is the run of pieces where the segment crosses one of its
//! triangles, so the surface is linear on each, and where a piece of one
//! surface meets a piece of the other the difference between them is linear
//! too. Each such span is a trapezoid (two triangles of opposite sign where
//! the surfaces cross inside it) in closed form. Nothing is sampled.
//!
//! THE VOLUMES ARE THE METHODS', AS STATED. Between stations s1 and s2,
//! L = s2 - s1 apart, with areas A1 and A2:
//!
//! ```text
//!   average end area  V = L (A1 + A2) / 2
//!   prismoidal        V = L (A1 + 4 Am + A2) / 6, Am the area of the section
//!                     cut at the middle station, not the mean of A1 and A2
//! ```
//!
//! both as Ghilani and Wolf give them (Elementary Surveying: An Introduction
//! to Geomatics, the chapter "Volumes": the average-end-area and prismoidal
//! formulas). The prismoidal formula is Simpson's rule applied to the area as
//! a function of station, so it is exact when that function is a polynomial
//! of degree three or less (Burden and Faires, Numerical Analysis, "Elements
//! of Numerical Integration": Simpson's rule has degree of precision three):
//! a section whose width and depth both change linearly along the job, whose
//! area is then quadratic, is measured exactly by it and overstated by the
//! average end area. The difference of the two is the prismoidal correction.
//!
//! THE CURVATURE CORRECTION (Pappus). L is measured along the centreline, but
//! on a curve the ground at offset o (left positive) is swept along an arc of
//! (1 - k o) times the centreline's, k the signed curvature (positive turning
//! left): a point on the inside of a curve travels less than the centreline,
//! one on the outside more. A section of area A whose centroid stands e from
//! the centreline therefore carries A (1 - k e) = A - k M for every metre of
//! station, M = A e the first moment of the section about the centreline, and
//! the volume between two normals is the integral of A(s) - k(s) M(s) ds.
//! Both formulas above are applied to A - k M in the place of A, the cut and
//! the fill of each section on their own (each has its own centroid), with k
//! the curvature at the middle of the panel: exact on an arc, and on a spiral,
//! where k is linear in station, exact for a section that does not change
//! along the panel. On a straight k is 0 and the figures are the plain ones;
//! so they are for a section symmetric about the centreline on any curve.
//! Left off, a quarter circle of radius 50 with a strip 10 m wide on its
//! inside measures 4.6 % over, and one on its outside 4.2 % under: the error
//! is the fixed factor R / (R -/+ e) of every panel, and a shorter interval
//! does not shrink it (docs/volumes.md, "Along an alignment").

use katana_core::error::make_error;
use katana_core::task_pool::TaskPool;
use katana_core::text::format_exact_real;
use katana_core::{Error, ErrorCode, Result};
use katana_geometry::alignment::SolvedAlignment;
use katana_geometry::{SpatialEntry, SpatialId, SpatialIndex, Triangle2};
use katana_math::cpp;
use katana_math::numerics::tolerance;

use crate::summation::CompensatedSum;
use crate::tin_surface::{Box2, Point2, TinSurface, compute_by_index};

// ---- along a segment -----------------------------------------------------------------------

/// Where a surface has a height along a plan segment: from t0 to t1 of the
/// segment's length (0 at its start, 1 at its end), inside one triangle, so
/// the height is linear between z0 and z1.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SegmentPiece {
    pub t0: f64,
    pub t1: f64,
    pub z0: f64,
    pub z1: f64,
}

/// A surface read along straight plan segments, exactly: the segment is
/// clipped by each triangle it crosses. Where it crosses an edge, the crossing
/// and its height are computed from the edge's own vertices (in a fixed
/// order), so the pieces on either side of an edge meet at the same bits.
///
/// A segment that lies ON an edge (both its ends within GEOMETRIC of the
/// edge's line, which is where a section cut at the end of a corridor lies on
/// the corridor surface's boundary) is on the ground along the whole of the
/// edge: the triangle beside the edge reports it, and so does the triangle on
/// the other side. The ends of a section are computed from a bearing, so a
/// north-going one is tilted by 1e-16 of its length, and an exact test would
/// have kept whichever half the tilt left inside.
#[derive(Clone, Debug)]
pub struct SegmentProfiler<'a> {
    surface: &'a TinSurface,
    index: SpatialIndex,
}

/// Where the segment from `a` along `d` crosses the line of the edge from u to
/// v: its parameter along the segment and the height along the edge there.
/// Computed from the edge's two vertices in a fixed order (the lower index
/// first), so both triangles that share the edge get the same bits.
#[derive(Clone, Copy, Debug, Default)]
struct EdgeCrossing {
    t: f64,
    z: f64,
}

fn cross_edge(surface: &TinSurface, first: u32, second: u32, a: Point2, d: Point2) -> EdgeCrossing {
    let low = first.min(second);
    let high = first.max(second);
    let u = surface.vertices()[low as usize];
    let v = surface.vertices()[high as usize];
    let ex = v.x - u.x;
    let ey = v.y - u.y;
    // A + t D = U + s (V - U): crossed with (V - U) gives t, with D gives s.
    let denominator = d.x * ey - d.y * ex;
    let t = ((u.x - a.x) * ey - (u.y - a.y) * ex) / denominator;
    let s = ((u.x - a.x) * d.y - (u.y - a.y) * d.x) / denominator;
    let along = cpp::clamp(s, 0.0, 1.0);
    EdgeCrossing {
        t,
        z: u.z * (1.0 - along) + v.z * along,
    }
}

impl<'a> SegmentProfiler<'a> {
    /// Indexes the surface's triangles once; the surface must outlive this.
    pub fn new(surface: &'a TinSurface) -> Self {
        let mut entries: Vec<SpatialEntry> = Vec::with_capacity(surface.triangle_count());
        for t in 0..surface.triangle_count() {
            let plan = surface.plan_triangle(t);
            // A degenerate triangle has no plane, and no length of a segment
            // lies inside it.
            if !plan.is_degenerate() {
                entries.push(SpatialEntry {
                    id: t as SpatialId,
                    box_value: plan.bounding_box(),
                });
            }
        }
        let mut index = SpatialIndex::default();
        index.rebuild(&entries);
        Self { surface, index }
    }

    /// The pieces along `from` to `to`, ascending t; a gap is where the
    /// segment is off the surface or in a hole. A segment of no length has
    /// none. Safe to call from several threads at once.
    pub fn along(&self, from: Point2, to: Point2) -> Vec<SegmentPiece> {
        let mut pieces: Vec<SegmentPiece> = Vec::new();
        let d = Point2::new(to.x - from.x, to.y - from.y);
        if d.x == 0.0 && d.y == 0.0 {
            return pieces;
        }
        let mut bounds = Box2::default();
        bounds.expand(from);
        bounds.expand(to);
        let candidates = self.index.query(bounds.inflated(tolerance::GEOMETRIC));
        let surface = self.surface;
        let plan_of = |v: u32| {
            let p = surface.vertices()[v as usize];
            Point2::new(p.x, p.y)
        };
        for &id in &candidates {
            let t = id as usize;
            let tri = surface.triangles()[t];
            let mut corner = tri;
            if Triangle2::new(plan_of(corner[0]), plan_of(corner[1]), plan_of(corner[2]))
                .signed_area()
                < 0.0
            {
                corner.swap(1, 2); // counter-clockwise, so inside is left
            }
            // Cyrus-Beck: each edge bounds the segment's parameter from below
            // where the segment enters across it and from above where it
            // leaves.
            let mut t0 = 0.0;
            let mut t1 = 1.0;
            let mut z0: Option<f64> = None;
            let mut z1: Option<f64> = None;
            let mut outside = false;
            for k in 0..3 {
                if outside {
                    break;
                }
                let p = plan_of(corner[k]);
                let q = plan_of(corner[(k + 1) % 3]);
                let ex = q.x - p.x;
                let ey = q.y - p.y;
                let rate = ex * d.y - ey * d.x; // how the side changes along the segment
                // How far each end of the segment stands from this edge's
                // line, positive on the inside.
                let edge_length = (ex * ex + ey * ey).sqrt();
                let side_from = (ex * (from.y - p.y) - ey * (from.x - p.x)) / edge_length;
                let side_to = (ex * (to.y - p.y) - ey * (to.x - p.x)) / edge_length;
                if side_from.abs() <= tolerance::GEOMETRIC && side_to.abs() <= tolerance::GEOMETRIC
                {
                    // The segment lies ON this edge: it is the surface's
                    // boundary or the seam between two triangles, and a section
                    // cut there is cut on the ground, not beside it. Its ends
                    // are computed from a bearing, so a north-going section is
                    // tilted by the rounding of cos(pi/2) = 6e-17 and an exact
                    // test would clip away whichever half the tilt put outside.
                    // Inside the half-plane, as the exactly parallel case was:
                    // the triangle's other two edges still bound it, and the
                    // triangle on the edge's other side reports the same span,
                    // which piece_at takes for one.
                    continue;
                }
                if rate == 0.0 {
                    // Parallel: wholly inside this edge's half-plane, or wholly out.
                    outside = side_from < 0.0;
                    continue;
                }
                let crossing = cross_edge(surface, corner[k], corner[(k + 1) % 3], from, d);
                if rate > 0.0 {
                    if crossing.t > t0 {
                        t0 = crossing.t;
                        z0 = Some(crossing.z);
                    }
                } else if crossing.t < t1 {
                    t1 = crossing.t;
                    z1 = Some(crossing.z);
                }
            }
            if outside || !(t1 > t0) {
                continue;
            }
            // An end inside the triangle (the segment starts or ends in it) is
            // read off the triangle's plane.
            let plane = |at: f64| {
                let position = Point2::new(from.x + at * d.x, from.y + at * d.y);
                let plan = surface.plan_triangle(t);
                let w = plan.barycentric_with_area(position, plan.twice_signed_area());
                w[0] * surface.vertices()[tri[0] as usize].z
                    + w[1] * surface.vertices()[tri[1] as usize].z
                    + w[2] * surface.vertices()[tri[2] as usize].z
            };
            pieces.push(SegmentPiece {
                t0,
                t1,
                z0: z0.unwrap_or_else(|| plane(t0)),
                z1: z1.unwrap_or_else(|| plane(t1)),
            });
        }
        // By t0, then t1. Every t0 kept is above +0 or is +0 itself and no
        // value is a NaN, so the total order is the C++'s `<`; a stable sort
        // keeps tied pieces in triangle order, where std::sort leaves the
        // order of ties unspecified.
        pieces.sort_by(|a, b| a.t0.total_cmp(&b.t0).then(a.t1.total_cmp(&b.t1)));
        pieces
    }
}

// ---- the area on one section ----------------------------------------------------------------

/// A trapezoid of width `width` whose heights at its two sides are d0 and d1,
/// its positive and negative parts kept apart: where d changes sign inside it,
/// two triangles split at the zero. The one sign-split of a linear strip.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SignedArea {
    pub positive: f64,
    /// as a magnitude
    pub negative: f64,
}

pub fn signed_trapezoid(d0: f64, d1: f64, width: f64) -> SignedArea {
    let mut area = SignedArea::default();
    if !(width > 0.0) {
        return area;
    }
    if d0 >= 0.0 && d1 >= 0.0 {
        area.positive = 0.5 * (d0 + d1) * width;
    } else if d0 <= 0.0 && d1 <= 0.0 {
        area.negative = -0.5 * (d0 + d1) * width;
    } else {
        // The two cross inside the strip: two triangles of opposite sign,
        // split where the difference is zero.
        let t = d0 / (d0 - d1);
        let left = d0.abs() * t * width * 0.5;
        let right = d1.abs() * (1.0 - t) * width * 0.5;
        if d0 > 0.0 {
            area.positive += left;
        } else {
            area.negative += left;
        }
        if d1 > 0.0 {
            area.positive += right;
        } else {
            area.negative += right;
        }
    }
    area
}

/// The first moments of the same two parts about a line in the plan of the
/// strip, in closed form: the integral of the depth times the offset from that
/// line. `offset_at_start` is the offset of the strip's first side from the
/// line, and the offset falls by one for every unit of width across the strip,
/// so the strip runs from the line's positive side towards its negative. The
/// negative part's depth is taken as a magnitude, so each moment is signed by
/// the offset alone: a part on the line's positive side has a positive moment
/// whichever part it is. The parts are the ones signed_trapezoid splits, the
/// same way, and a moment divided by its area is the part's centroid offset.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SignedMoment {
    pub positive: f64,
    pub negative: f64,
}

pub fn signed_trapezoid_moments(
    d0: f64,
    d1: f64,
    width: f64,
    offset_at_start: f64,
) -> SignedMoment {
    let mut moment = SignedMoment::default();
    if !(width > 0.0) {
        return moment;
    }
    // A strip of width w whose depth runs linearly from a to b (magnitudes),
    // its first side at offset o and the offset falling with u across it:
    //   integral over u in [0, w] of (a + (b - a) u / w) (o - u) du
    //     = o w (a + b) / 2 - w^2 (a / 6 + b / 3).
    let whole = |a: f64, b: f64| {
        offset_at_start * width * 0.5 * (a + b) - width * width * (a / 6.0 + b / 3.0)
    };
    // The same three cases as signed_trapezoid, in the same order, so a part
    // that has an area has a moment and one that has none has none.
    if d0 >= 0.0 && d1 >= 0.0 {
        moment.positive = whole(d0, d1);
    } else if d0 <= 0.0 && d1 <= 0.0 {
        moment.negative = whole(-d0, -d1);
    } else {
        // Two triangles of opposite sign split where the depth is zero: the
        // first of width t w with its apex of depth |d0| at the first side,
        // the second of width (1 - t) w with |d1| at the far side. A
        // triangle's centroid stands a third of its base from its tall side.
        let t = d0 / (d0 - d1);
        let first_width = t * width;
        let second_width = (1.0 - t) * width;
        let first_area = d0.abs() * first_width * 0.5;
        let second_area = d1.abs() * second_width * 0.5;
        let first = first_area * (offset_at_start - first_width / 3.0);
        let second = second_area * (offset_at_start - width + second_width / 3.0);
        if d0 > 0.0 {
            moment.positive += first;
        } else {
            moment.negative += first;
        }
        if d1 > 0.0 {
            moment.positive += second;
        } else {
            moment.negative += second;
        }
    }
    moment
}

/// The piece of `pieces` (ascending t0) that holds `t`, or None in a gap.
fn piece_at(pieces: &[SegmentPiece], t: f64) -> Option<&SegmentPiece> {
    // std::upper_bound: the first piece that starts after `t`.
    let mut after = pieces.partition_point(|piece| !(t < piece.t0));
    // Every piece before `after` starts at or before `t`. Pieces of one
    // surface overlap by rounding at most (and repeat where the segment runs
    // along an edge, which both triangles beside it report), so the last two
    // are the only ones that can still reach it.
    for _ in 0..2 {
        if after == 0 {
            break;
        }
        after -= 1;
        if t <= pieces[after].t1 {
            return Some(&pieces[after]);
        }
    }
    None
}

/// Linear on the piece; exact at both of its ends.
fn height_on(piece: &SegmentPiece, t: f64) -> f64 {
    if t <= piece.t0 {
        return piece.z0;
    }
    if t >= piece.t1 {
        return piece.z1;
    }
    let w = (t - piece.t0) / (piece.t1 - piece.t0);
    piece.z0 * (1.0 - w) + piece.z1 * w
}

/// The area between two surfaces on one section, of plan length `length`:
/// fill where `design` is above `existing`, cut where it is below, as
/// compare_surfaces names them. `width` is the plan length on which both have
/// a height: what was measured. `cut_moment` and `fill_moment` are the areas'
/// first moments about a line on the section: the point `offset_at_start`
/// from the line at its first end, the offset falling along the section
/// (signed_trapezoid_moments). 0 is the first end itself.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SectionAreas {
    pub cut: f64,
    pub fill: f64,
    pub width: f64,
    pub cut_moment: f64,
    pub fill_moment: f64,
}

/// The C++'s `offsetAtStart` defaults to 0, the first end itself.
pub fn section_areas(
    existing: &[SegmentPiece],
    design: &[SegmentPiece],
    length: f64,
    offset_at_start: f64,
) -> SectionAreas {
    let mut areas = SectionAreas::default();
    if existing.is_empty() || design.is_empty() || !(length > 0.0) {
        return areas;
    }
    // Every place either surface changes slope; between two of them both are
    // linear, and so is their difference.
    let mut breaks: Vec<f64> = Vec::with_capacity(2 * (existing.len() + design.len()));
    for pieces in [existing, design] {
        for piece in pieces {
            breaks.push(piece.t0);
            breaks.push(piece.t1);
        }
    }
    // Every t is a finite parameter that is not -0 (see along), so the total
    // order is the C++'s `<`.
    breaks.sort_by(f64::total_cmp);
    breaks.dedup();

    let mut cut = CompensatedSum::new();
    let mut fill = CompensatedSum::new();
    let mut width = CompensatedSum::new();
    let mut cut_moment = CompensatedSum::new();
    let mut fill_moment = CompensatedSum::new();
    for pair in breaks.windows(2) {
        let ta = pair[0];
        let tb = pair[1];
        let middle = 0.5 * (ta + tb);
        let (Some(under), Some(over)) = (piece_at(existing, middle), piece_at(design, middle))
        else {
            continue; // one of them has no height here: nothing is measured
        };
        let strip = (tb - ta) * length;
        let d0 = height_on(over, ta) - height_on(under, ta);
        let d1 = height_on(over, tb) - height_on(under, tb);
        let area = signed_trapezoid(d0, d1, strip);
        fill.add(area.positive);
        cut.add(area.negative);
        width.add(strip);
        // The strip starts ta along the section, so its first side is that
        // much nearer the section's far end than the section's first end is.
        let moment = signed_trapezoid_moments(d0, d1, strip, offset_at_start - ta * length);
        fill_moment.add(moment.positive);
        cut_moment.add(moment.negative);
    }
    areas.cut = cut.value();
    areas.fill = fill.value();
    areas.width = width.value();
    areas.cut_moment = cut_moment.value();
    areas.fill_moment = fill_moment.value();
    areas
}

// ---- along an alignment ---------------------------------------------------------------------

/// The more sections than this is a unit mistake, not a wish: the cap
/// terrain::contours puts on its levels (docs/terrain.md, "Contours").
pub const MAX_VOLUME_SECTIONS: usize = 100_000;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AlignmentVolumeOptions {
    /// Sections at every whole multiple of this station (10, 20, 30 ... for
    /// 10), plus the range's ends and the alignment's key stations inside it.
    pub interval: f64,
    /// The station range; absent: the alignment's start and end.
    pub from: Option<f64>,
    pub to: Option<f64>,
    /// How far each section reaches left and right of the centreline (left is
    /// left looking along increasing station). Absent: the diagonal of the
    /// ground the two surfaces have in common, so a section reaches across
    /// all of it wherever it is cut, which on a straight run is right, and
    /// on a curve or a corner is refused: see volumes_along_alignment.
    pub left: Option<f64>,
    pub right: Option<f64>,
    /// Also cut the section at the middle of each pair, for the prismoidal
    /// formula.
    pub prismoidal: bool,
    /// The curvature correction above. On by default: it is the standard
    /// method, and it changes nothing on a straight or for a section that is
    /// symmetric about the centreline. Off gives the plain figure, the areas
    /// along the centreline.
    pub curvature: bool,
}

impl Default for AlignmentVolumeOptions {
    fn default() -> Self {
        Self {
            interval: 10.0,
            from: None,
            to: None,
            left: None,
            right: None,
            prismoidal: false,
            curvature: true,
        }
    }
}

/// One cross section.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StationSection {
    pub station: f64,
    pub centre: Point2,
    pub cut_area: f64,
    pub fill_area: f64,
    /// The areas' first moments about the centreline, left positive: the cut
    /// area times its centroid's offset, and the fill area times its own.
    pub cut_moment: f64,
    pub fill_moment: f64,
    /// plan length on which both surfaces have a height
    pub width: f64,
    /// An end of the range or a key station of the alignment (TS, SC, CS,
    /// ST): where the geometry changes, so a section is always cut there.
    pub key: bool,
}

/// What the curvature correction added to a volume (negative: took away), cut
/// and fill separately: the volume as reported less the plain one, the areas
/// along the centreline.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CurvatureShift {
    pub cut: f64,
    pub fill: f64,
}

/// Between two consecutive sections.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SectionPanel {
    pub from_station: f64,
    pub to_station: f64,
    /// As reported: with the curvature correction when it is on.
    pub end_area_cut: f64,
    pub end_area_fill: f64,
    /// With AlignmentVolumeOptions::prismoidal: the middle section and the
    /// prismoidal volumes.
    pub middle: Option<StationSection>,
    pub prismoidal_cut: f64,
    pub prismoidal_fill: f64,
    /// The signed curvature at the middle of the panel, 1 / m, positive
    /// turning left; 0 on a straight and when the correction is off.
    pub curvature: f64,
    pub end_area_shift: CurvatureShift,
    /// with prismoidal
    pub prismoidal_shift: CurvatureShift,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AlignmentVolumes {
    /// ascending station
    pub sections: Vec<StationSection>,
    /// sections.len() - 1 of them
    pub panels: Vec<SectionPanel>,
    /// As reported: with the curvature correction when it is on.
    pub end_area_cut: f64,
    pub end_area_fill: f64,
    /// with prismoidal
    pub prismoidal_cut: f64,
    pub prismoidal_fill: f64,
    /// The panels' shifts, summed: what the correction did to each total.
    pub end_area_shift: CurvatureShift,
    pub prismoidal_shift: CurvatureShift,
    /// the correction was made
    pub curvature: bool,
    pub from_station: f64,
    pub to_station: f64,
    pub left: f64,
    pub right: f64,
    pub prismoidal: bool,
}

impl Default for AlignmentVolumes {
    fn default() -> Self {
        Self {
            sections: Vec::new(),
            panels: Vec::new(),
            end_area_cut: 0.0,
            end_area_fill: 0.0,
            prismoidal_cut: 0.0,
            prismoidal_fill: 0.0,
            end_area_shift: CurvatureShift::default(),
            prismoidal_shift: CurvatureShift::default(),
            curvature: true,
            from_station: 0.0,
            to_station: 0.0,
            left: 0.0,
            right: 0.0,
            prismoidal: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Station {
    station: f64,
    key: bool,
}

/// `std::format("{:.3f}", value)`: Rust's fixed digits are the C++'s for every
/// finite value and an infinity, and both keep the sign of a value that rounds
/// to zero ("-0.000"); a NaN is spelt "nan" or "-nan" where Rust writes "NaN".
fn fixed3(value: f64) -> String {
    if value.is_nan() {
        return String::from(if value.is_sign_negative() {
            "-nan"
        } else {
            "nan"
        });
    }
    format!("{value:.3}")
}

/// A box as a refusal names it: the corners to the millimetre.
fn box_text(bounds: &Box2) -> String {
    format!(
        "{},{},{},{}",
        fixed3(bounds.min.x),
        fixed3(bounds.min.y),
        fixed3(bounds.max.x),
        fixed3(bounds.max.y)
    )
}

/// A section's two ends: left looking along increasing station, then right.
#[derive(Clone, Copy, Debug, Default)]
struct SectionEnds {
    left: Point2,
    right: Point2,
    valid: bool,
}

/// Where two sections properly cross: the fraction of each from its left end.
/// Meeting at an end within GEOMETRIC is not crossing: sections cut as far as
/// a curve's radius all end on its centre, and that is as far as they may go.
/// Parallel sections (the sections of a straight run) never cross.
fn crossing_of(a: &SectionEnds, b: &SectionEnds) -> Option<(f64, f64)> {
    let rx = a.right.x - a.left.x;
    let ry = a.right.y - a.left.y;
    let sx = b.right.x - b.left.x;
    let sy = b.right.y - b.left.y;
    let denominator = rx * sy - ry * sx;
    if denominator == 0.0 {
        return None;
    }
    // a.left + t r = b.left + u s, crossed with s and with r.
    let qx = b.left.x - a.left.x;
    let qy = b.left.y - a.left.y;
    let t = (qx * sy - qy * sx) / denominator;
    let u = (qx * ry - qy * rx) / denominator;
    let margin_a = tolerance::GEOMETRIC / rx.hypot(ry);
    let margin_b = tolerance::GEOMETRIC / sx.hypot(sy);
    if t > margin_a && t < 1.0 - margin_a && u > margin_b && u < 1.0 - margin_b {
        return Some((t, u));
    }
    None
}

#[derive(Clone, Copy, Debug)]
struct SectionCrossing {
    first: usize,
    second: usize,
    /// The longest reach, the same each side, at which no pair of sections
    /// crosses: for a crossing, the greater of its two distances from the
    /// centreline (cut shorter than that, one of the two sections has ended
    /// before the other is reached), and the least over every crossing.
    nearest: f64,
}

/// One section's share of the crossing test.
#[derive(Clone, Copy, Debug)]
struct CrossingSlot {
    found: bool,
    second: usize,
    nearest: f64,
}

impl Default for CrossingSlot {
    fn default() -> Self {
        Self {
            found: false,
            second: 0,
            nearest: f64::INFINITY,
        }
    }
}

/// The first pair of sections (in station order) that cross, and how short a
/// reach clears every crossing. The end-area method takes the volume of a
/// panel from the areas of the two sections that bound it, and is right only
/// while the panels tile the ground: where sections cross, the ground beyond
/// the crossing is swept by both, and counted by both.
fn first_crossing(
    ends: &[SectionEnds],
    count: usize,
    left: f64,
    right: f64,
    workers: &TaskPool,
) -> Option<SectionCrossing> {
    let box_of = |i: usize| {
        let mut bounds = Box2::default();
        bounds.expand(ends[i].left);
        bounds.expand(ends[i].right);
        bounds.inflated(tolerance::GEOMETRIC)
    };
    let entries: Vec<SpatialEntry> = (0..count)
        .filter(|&i| ends[i].valid)
        .map(|i| SpatialEntry {
            id: i as SpatialId,
            box_value: box_of(i),
        })
        .collect();
    let mut index = SpatialIndex::default();
    index.rebuild(&entries);

    let reach = left + right;
    let slots = compute_by_index(workers, count, 16, CrossingSlot::default(), |i| {
        let mut slot = CrossingSlot::default();
        if !ends[i].valid {
            return slot;
        }
        for id in index.query(box_of(i)) {
            let j = id as usize;
            if j <= i {
                continue;
            }
            let Some((first, second)) = crossing_of(&ends[i], &ends[j]) else {
                continue;
            };
            // The distance of the crossing from the centreline along each
            // section: a section runs from `left` on one side of it to
            // `right` on the other.
            let on_first = (left - first * reach).abs();
            let on_second = (left - second * reach).abs();
            if !slot.found {
                slot.found = true;
                slot.second = j;
            }
            slot.nearest = cpp::min(slot.nearest, cpp::max(on_first, on_second));
        }
        slot
    });

    // Combined in station order, so the pair named and the reach given are
    // the same at any thread count.
    let mut result: Option<SectionCrossing> = None;
    for (i, slot) in slots.iter().enumerate() {
        if !slot.found {
            continue;
        }
        result = Some(match result {
            None => SectionCrossing {
                first: i,
                second: slot.second,
                nearest: slot.nearest,
            },
            Some(found) => SectionCrossing {
                nearest: cpp::min(found.nearest, slot.nearest),
                ..found
            },
        });
    }
    result
}

// ---- stretches that run side by side ---------------------------------------------------------

type Tri = [Point2; 3];

/// Twice the signed area of the triangle o, a, b.
fn cross_of(o: Point2, a: Point2, b: Point2) -> f64 {
    (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x)
}

/// The ground one panel sweeps: the quadrilateral between the reach segments
/// of its two sections, as two triangles that tile it. A panel across a corner
/// can be concave, and the diagonal from a corner that turns the wrong way
/// would cover ground outside it; so the diagonal is the one whose two
/// triangles both turn the way the quadrilateral does, and the other when the
/// first fails.
#[derive(Clone, Copy, Debug, Default)]
struct Strip {
    parts: [Tri; 2],
    bounds: Box2,
    valid: bool,
}

fn strip_of(a: &SectionEnds, b: &SectionEnds) -> Strip {
    let mut strip = Strip::default();
    if !a.valid || !b.valid {
        return strip;
    }
    // Round the quadrilateral: a's left, a's right, b's right, b's left.
    let p = a.left;
    let q = a.right;
    let r = b.right;
    let s = b.left;
    let whole = cross_of(p, q, r) + cross_of(p, r, s);
    let turns_with_it = |part: f64| {
        if whole >= 0.0 {
            part >= 0.0
        } else {
            part <= 0.0
        }
    };
    if turns_with_it(cross_of(p, q, r)) && turns_with_it(cross_of(p, r, s)) {
        strip.parts = [[p, q, r], [p, r, s]];
    } else if turns_with_it(cross_of(q, r, s)) && turns_with_it(cross_of(q, s, p)) {
        strip.parts = [[q, r, s], [q, s, p]];
    } else {
        // Its sides cross, which sections that do not cross cannot make but a
        // fold of more than a half turn in one panel might: the ground it
        // covers is not defined, and the first diagonal is as good a reading.
        strip.parts = [[p, q, r], [p, r, s]];
    }
    for corner in [p, q, r, s] {
        strip.bounds.expand(corner);
    }
    strip.bounds = strip.bounds.inflated(tolerance::GEOMETRIC);
    strip.valid = true;
    strip
}

/// Whether two triangles interpenetrate by more than `depth`: the separating
/// axis test, whose least overlap of the two shadows over the normals of the
/// edges is the shortest way one would have to move to clear the other. A
/// triangle of no area has an axis along which it is a point, so it never
/// overlaps by anything.
fn overlaps_deeper_than(first: &Tri, second: &Tri, depth: f64) -> bool {
    for owner in [first, second] {
        for k in 0..3 {
            let from = owner[k];
            let to = owner[(k + 1) % 3];
            let ex = to.x - from.x;
            let ey = to.y - from.y;
            let length = ex.hypot(ey);
            if !(length > 0.0) {
                continue;
            }
            let nx = -ey / length;
            let ny = ex / length;
            // Relative to the edge's own end, so a survey coordinate's size
            // does not cost the projection its digits.
            let shadow = |triangle: &Tri| {
                let mut low = f64::INFINITY;
                let mut high = -low;
                for corner in triangle {
                    let at = (corner.x - from.x) * nx + (corner.y - from.y) * ny;
                    low = cpp::min(low, at);
                    high = cpp::max(high, at);
                }
                (low, high)
            };
            let (low_a, high_a) = shadow(first);
            let (low_b, high_b) = shadow(second);
            if !(cpp::min(high_a, high_b) - cpp::max(low_a, low_b) > depth) {
                return false;
            }
        }
    }
    true
}

#[derive(Clone, Copy, Debug, Default)]
struct SideBySide {
    /// The first stretch: the panels from `first` to `last`, a run of
    /// consecutive panels that each overlap some other panel but their
    /// neighbours.
    first: usize,
    last: usize,
    /// The panels they overlap, the lowest to the highest.
    partner_first: usize,
    partner_last: usize,
    /// Pairs of panels that overlap, all told.
    pairs: usize,
}

/// One panel's share of the side-by-side test.
#[derive(Clone, Copy, Debug, Default)]
struct SideSlot {
    pairs: usize,
    /// the lowest and highest partner above p + 1
    first: usize,
    last: usize,
}

/// Whether two panels that are not neighbours sweep common ground. Neighbours
/// share a section, so they touch along it; a panel and the one after next
/// are tested like any other, and clear one another when the sections tile
/// the ground. `count` sections give count - 1 panels.
fn first_side_by_side(
    ends: &[SectionEnds],
    count: usize,
    workers: &TaskPool,
) -> Option<SideBySide> {
    if count < 4 {
        return None; // three panels are the fewest with two that are not neighbours
    }
    let panels = count - 1;
    let strips: Vec<Strip> = (0..panels)
        .map(|p| strip_of(&ends[p], &ends[p + 1]))
        .collect();
    let entries: Vec<SpatialEntry> = strips
        .iter()
        .enumerate()
        .filter(|(_, strip)| strip.valid)
        .map(|(p, strip)| SpatialEntry {
            id: p as SpatialId,
            box_value: strip.bounds,
        })
        .collect();
    let mut index = SpatialIndex::default();
    index.rebuild(&entries);

    let slots = compute_by_index(workers, panels, 16, SideSlot::default(), |p| {
        let mut slot = SideSlot::default();
        if !strips[p].valid {
            return slot;
        }
        for id in index.query(strips[p].bounds) {
            let q = id as usize;
            if q < p + 2 {
                continue; // itself, or a neighbour; the pair is counted from the lower
            }
            let mut overlap = false;
            for a in &strips[p].parts {
                for b in &strips[q].parts {
                    overlap = overlap || overlaps_deeper_than(a, b, tolerance::GEOMETRIC);
                }
            }
            if !overlap {
                continue;
            }
            slot.first = if slot.pairs == 0 {
                q
            } else {
                slot.first.min(q)
            };
            slot.last = if slot.pairs == 0 { q } else { slot.last.max(q) };
            slot.pairs += 1;
        }
        slot
    });

    // Combined in station order, so the stretches named are the same at any
    // thread count.
    let mut found = SideBySide::default();
    let mut any = false;
    for (p, slot) in slots.iter().enumerate() {
        if slot.pairs == 0 {
            continue;
        }
        if !any {
            found = SideBySide {
                first: p,
                last: p,
                partner_first: slot.first,
                partner_last: slot.last,
                pairs: slot.pairs,
            };
            any = true;
            continue;
        }
        found.pairs += slot.pairs;
        if p == found.last + 1 {
            // The run goes on only while the panels are consecutive; once a
            // panel is missed no later one is next after `last`.
            found.last = p;
            found.partner_first = found.partner_first.min(slot.first);
            found.partner_last = found.partner_last.max(slot.last);
        }
    }
    any.then_some(found)
}

/// Sections between `existing` and `design` along `alignment`. Fails with
/// InvalidArgument for an empty surface, an interval that is not finite and
/// positive, a range outside the alignment or of no length, a negative or
/// non-finite reach or none at all, more than MAX_VOLUME_SECTIONS sections, or
/// sections that cross one another; with InvalidGeometry when the surfaces
/// have no ground in common (their boxes do not meet, or no section of the
/// range reaches ground that both have).
///
/// The average end area is the volume of a panel from the areas of the two
/// sections that bound it, and is right only while the panels tile the
/// ground. Where two sections cross (the inside of a curve cut deeper than its
/// radius, the inside of a corner cut deeper than the interval over the sine
/// of its deflection, the legs either side of a bend) the ground beyond the
/// crossing is swept by both and counted by both (a 1 m slab of 40 000 m3
/// measured 62 101 m3 round a curve of radius 40 with the default reach). That
/// is refused, naming the stations and the longest reach that clears every
/// crossing, rather than measured; sections that only touch at their ends (a
/// reach equal to the radius) are not crossing.
///
/// Two stretches of the alignment running side by side nearer than the reach
/// (the legs of a U-turn made of corners, two roads laid in one alignment)
/// sweep the same ground twice with no section of one crossing a section of
/// the other (their sections are parallel). So the strip each panel sweeps,
/// the quadrilateral between the reach segments of its two sections, is also
/// tested against the strip of every panel but its neighbours (which share a
/// section); a pair that overlaps by more than GEOMETRIC is refused
/// (InvalidArgument), naming the stations of the two stretches, and nothing is
/// measured. Strips that only touch are not overlapping.
///
/// Sections are cut on `pool` (None: TaskPool::shared()), each on its own, and
/// summed in station order: the same bits at any thread count, as the two
/// tests above find the same pair at any thread count.
pub fn volumes_along_alignment(
    existing: &TinSurface,
    design: &TinSurface,
    alignment: &SolvedAlignment,
    options: &AlignmentVolumeOptions,
    pool: Option<&TaskPool>,
) -> Result<AlignmentVolumes> {
    if existing.empty() || design.empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "both surfaces must have triangles",
        ));
    }
    if !(options.interval > 0.0) || !options.interval.is_finite() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the interval must be a positive length",
        ));
    }
    let start = alignment.start_station();
    let end = alignment.end_station();
    let from = options.from.unwrap_or(start);
    let to = options.to.unwrap_or(end);
    if !from.is_finite() || !to.is_finite() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the stations must be finite",
        ));
    }
    if from < start - tolerance::COORDINATE || to > end + tolerance::COORDINATE {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!(
                "the range {} to {} is outside the alignment, which runs {} to {}",
                format_exact_real(from),
                format_exact_real(to),
                format_exact_real(start),
                format_exact_real(end)
            ),
        ));
    }
    if !(to - from > tolerance::COORDINATE) {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "the range has no length: from must be before to",
        ));
    }
    let first = cpp::max(from, start);
    let last = cpp::min(to, end);
    if (last - first) / options.interval > MAX_VOLUME_SECTIONS as f64 {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!(
                "an interval of {} over {} makes more than {MAX_VOLUME_SECTIONS} sections",
                format_exact_real(options.interval),
                format_exact_real(last - first)
            ),
        ));
    }
    if !existing.bounds().intersects(*design.bounds()) {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            format!(
                "the existing surface covers {} and the design covers {}: they have no ground in common",
                box_text(existing.bounds()),
                box_text(design.bounds())
            ),
        ));
    }
    let common = Box2::new(
        Point2::new(
            cpp::max(existing.bounds().min.x, design.bounds().min.x),
            cpp::max(existing.bounds().min.y, design.bounds().min.y),
        ),
        Point2::new(
            cpp::min(existing.bounds().max.x, design.bounds().max.x),
            cpp::min(existing.bounds().max.y, design.bounds().max.y),
        ),
    );
    let reach = common.width().hypot(common.height());
    let left = options.left.unwrap_or(reach);
    let right = options.right.unwrap_or(reach);
    if !left.is_finite() || !right.is_finite() || left < 0.0 || right < 0.0 {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "left= and right= are distances from the centreline, 0 or more",
        ));
    }
    if !(left + right > 0.0) {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "a section must reach to one side at least: left= or right=",
        ));
    }

    // The stations: the ends, every whole multiple of the interval between,
    // and the alignment's key stations; two closer than COORDINATE (0.1 mm)
    // are one section, a key one if either was.
    let mut stations = vec![
        Station {
            station: first,
            key: true,
        },
        Station {
            station: last,
            key: true,
        },
    ];
    let mut k = (first / options.interval).floor() + 1.0;
    while k * options.interval < last {
        stations.push(Station {
            station: k * options.interval,
            key: false,
        });
        k += 1.0;
    }
    for key in alignment.key_stations() {
        if key > first && key < last {
            stations.push(Station {
                station: key,
                key: true,
            });
        }
    }
    // Every station is finite; ties merge below whatever their order.
    stations.sort_by(|a, b| a.station.total_cmp(&b.station));
    let mut merged: Vec<Station> = Vec::new();
    for station in stations {
        if let Some(back) = merged.last_mut()
            && station.station - back.station < tolerance::COORDINATE
        {
            back.key = back.key || station.key;
            continue;
        }
        merged.push(station);
    }
    if merged.len() > MAX_VOLUME_SECTIONS {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!("more than {MAX_VOLUME_SECTIONS} sections"),
        ));
    }

    // Every section to cut: the stations, then the middles for the
    // prismoidal formula.
    let mut cuts: Vec<f64> = Vec::with_capacity(2 * merged.len());
    cuts.extend(merged.iter().map(|station| station.station));
    if options.prismoidal {
        for pair in merged.windows(2) {
            cuts.push(0.5 * (pair[0].station + pair[1].station));
        }
    }

    // Both ends of every section, once: the crossing test and the cutting
    // both read them.
    let ends: Vec<SectionEnds> = cuts
        .iter()
        .map(|&cut| {
            match (
                alignment.point_at_station_offset(cut, left),
                alignment.point_at_station_offset(cut, -right),
            ) {
                // inside the range by construction
                (Some(left_end), Some(right_end)) => SectionEnds {
                    left: left_end,
                    right: right_end,
                    valid: true,
                },
                _ => SectionEnds::default(),
            }
        })
        .collect();
    let workers: &TaskPool = match pool {
        Some(pool) => pool,
        None => TaskPool::shared(),
    };
    if let Some(crossing) = first_crossing(&ends, merged.len(), left, right, workers) {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!(
                "the sections at stations {} and {} cross each other: sections that overlap count \
                 the ground they share more than once, so no volume can be given. Give left= and \
                 right= of at most {} m (on a curve, no more than its radius; at a corner with no \
                 radius, no more than the interval over the sine of its deflection)",
                fixed3(merged[crossing.first].station),
                fixed3(merged[crossing.second].station),
                fixed3(crossing.nearest)
            ),
            "left=,right=",
        ));
    }
    // No section crosses another, but two stretches may still run side by
    // side, their sections parallel and the ground between them swept by both.
    if let Some(side_by_side) = first_side_by_side(&ends, merged.len(), workers) {
        let station = |index: usize| fixed3(merged[index].station);
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!(
                "the stretch of the alignment from station {} to {} runs side by side with the \
                 stretch from station {} to {}: the sections of the two sweep the same ground ({} \
                 pairs of panels overlap), which would be counted once for each, so no volume can \
                 be given. Shorten left= and right= until the sections of the two stretches clear \
                 one another (with the same reach each side, under half the distance between the \
                 stretches)",
                station(side_by_side.first),
                station(side_by_side.last + 1),
                station(side_by_side.partner_first),
                station(side_by_side.partner_last + 1),
                side_by_side.pairs
            ),
            "left=,right=",
        ));
    }

    let under = SegmentProfiler::new(existing);
    let over = SegmentProfiler::new(design);
    // A section is a few hundred triangle clips; four to a task keeps the
    // dispatch small beside them. The grain cannot change a result: each
    // section is computed on its own.
    let mut sections = compute_by_index(workers, cuts.len(), 4, StationSection::default(), |i| {
        let mut section = StationSection {
            station: cuts[i],
            ..StationSection::default()
        };
        let centre = alignment.point_at_station(cuts[i]);
        let Some(centre) = centre else {
            return section; // inside the range by construction; nothing to cut otherwise
        };
        if !ends[i].valid {
            return section;
        }
        section.centre = centre;
        let left_end = ends[i].left;
        let right_end = ends[i].right;
        let length = left_end.distance_to(right_end);
        // The section runs from the left end, `left` from the centreline, to
        // the right end: its moments are about the centreline.
        let areas = section_areas(
            &under.along(left_end, right_end),
            &over.along(left_end, right_end),
            length,
            left,
        );
        section.cut_area = areas.cut;
        section.fill_area = areas.fill;
        section.cut_moment = areas.cut_moment;
        section.fill_moment = areas.fill_moment;
        section.width = areas.width;
        section
    });

    // Every other VOLUME form refuses surfaces that share no ground under what
    // it measures. Boxes that meet over triangles that do not, or an alignment
    // that runs clear of the common ground, measured nothing and would
    // otherwise answer a cut of 0 and a fill of 0.
    if !sections[..merged.len()]
        .iter()
        .any(|section| section.width > 0.0)
    {
        return Err(Error::new(
            ErrorCode::InvalidGeometry,
            format!(
                "no section of the range reaches ground that both surfaces have (the existing \
                 surface covers {}, the design {}): the alignment runs clear of it, or the two \
                 share no triangle there",
                box_text(existing.bounds()),
                box_text(design.bounds())
            ),
        ));
    }

    let mut result = AlignmentVolumes {
        from_station: first,
        to_station: last,
        left,
        right,
        prismoidal: options.prismoidal,
        curvature: options.curvature,
        ..AlignmentVolumes::default()
    };
    for (section, station) in sections.iter_mut().zip(&merged) {
        section.key = station.key;
        result.sections.push(*section);
    }
    let mut end_cut = CompensatedSum::new();
    let mut end_fill = CompensatedSum::new();
    let mut prism_cut = CompensatedSum::new();
    let mut prism_fill = CompensatedSum::new();
    let mut end_cut_shift = CompensatedSum::new();
    let mut end_fill_shift = CompensatedSum::new();
    let mut prism_cut_shift = CompensatedSum::new();
    let mut prism_fill_shift = CompensatedSum::new();
    for i in 0..result.sections.len().saturating_sub(1) {
        let a = result.sections[i];
        let b = result.sections[i + 1];
        let mut panel = SectionPanel {
            from_station: a.station,
            to_station: b.station,
            ..SectionPanel::default()
        };
        let length = b.station - a.station;
        // The curvature at the middle of the panel: a panel lies on one
        // element of the alignment (its key stations are all sectioned), so
        // that is the arc's own, 0 on a tangent, and on a spiral the mean of
        // its two ends'. Where the alignment kinks (a PI with no radius) there
        // is no curvature to speak of and 0 is what it has on either side.
        let bend = if options.curvature {
            alignment
                .curvature_at_station(0.5 * (a.station + b.station))
                .unwrap_or(0.0)
        } else {
            0.0
        };
        panel.curvature = bend;
        // Each section carries A - k M in the place of its area A (see the
        // module comment): plain when k is 0, which is the same expression as
        // ever.
        panel.end_area_cut = length * (a.cut_area + b.cut_area) / 2.0;
        panel.end_area_fill = length * (a.fill_area + b.fill_area) / 2.0;
        if bend != 0.0 {
            let cut =
                length * ((a.cut_area + b.cut_area) - bend * (a.cut_moment + b.cut_moment)) / 2.0;
            let fill = length
                * ((a.fill_area + b.fill_area) - bend * (a.fill_moment + b.fill_moment))
                / 2.0;
            panel.end_area_shift = CurvatureShift {
                cut: cut - panel.end_area_cut,
                fill: fill - panel.end_area_fill,
            };
            panel.end_area_cut = cut;
            panel.end_area_fill = fill;
        }
        end_cut.add(panel.end_area_cut);
        end_fill.add(panel.end_area_fill);
        end_cut_shift.add(panel.end_area_shift.cut);
        end_fill_shift.add(panel.end_area_shift.fill);
        if options.prismoidal {
            let m = sections[merged.len() + i];
            panel.middle = Some(m);
            panel.prismoidal_cut = length * (a.cut_area + 4.0 * m.cut_area + b.cut_area) / 6.0;
            panel.prismoidal_fill = length * (a.fill_area + 4.0 * m.fill_area + b.fill_area) / 6.0;
            if bend != 0.0 {
                let cut = length
                    * ((a.cut_area + 4.0 * m.cut_area + b.cut_area)
                        - bend * (a.cut_moment + 4.0 * m.cut_moment + b.cut_moment))
                    / 6.0;
                let fill = length
                    * ((a.fill_area + 4.0 * m.fill_area + b.fill_area)
                        - bend * (a.fill_moment + 4.0 * m.fill_moment + b.fill_moment))
                    / 6.0;
                panel.prismoidal_shift = CurvatureShift {
                    cut: cut - panel.prismoidal_cut,
                    fill: fill - panel.prismoidal_fill,
                };
                panel.prismoidal_cut = cut;
                panel.prismoidal_fill = fill;
            }
            prism_cut.add(panel.prismoidal_cut);
            prism_fill.add(panel.prismoidal_fill);
            prism_cut_shift.add(panel.prismoidal_shift.cut);
            prism_fill_shift.add(panel.prismoidal_shift.fill);
        }
        result.panels.push(panel);
    }
    result.end_area_cut = end_cut.value();
    result.end_area_fill = end_fill.value();
    result.prismoidal_cut = prism_cut.value();
    result.prismoidal_fill = prism_fill.value();
    result.end_area_shift = CurvatureShift {
        cut: end_cut_shift.value(),
        fill: end_fill_shift.value(),
    };
    result.prismoidal_shift = CurvatureShift {
        cut: prism_cut_shift.value(),
        fill: prism_fill_shift.value(),
    };
    Ok(result)
}
