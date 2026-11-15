//! Twin of tests/terrain/test_section_volume.cpp: volumes from cross sections
//! along an alignment. The surface read along a segment, the area between two
//! surfaces on a section, and the average end area and prismoidal volumes. The
//! expected values are worked by hand in the comments; the wedge's are also the
//! exact overlay's. Sections on a surface's boundary and sections that cross
//! are at the end.

pub mod support;

use katana_core::task_pool::TaskPool;
use katana_core::{ErrorCode, Result};
use katana_geometry::alignment::{
    AlignmentPi, HorizontalAlignment, SolvedAlignment, solve_alignment,
};
use katana_geometry::{Point2, Point3};
use katana_math::numerics::{DEG_TO_RAD, PI};
use katana_terrain::section_volume::{
    AlignmentVolumeOptions, AlignmentVolumes, SegmentPiece, SegmentProfiler, section_areas,
    signed_trapezoid, signed_trapezoid_moments, volumes_along_alignment,
};
use katana_terrain::volume::compare_surfaces;
use katana_terrain::{TinInput, TinSurface};
use support::*;

fn must<T>(result: Result<T>) -> T {
    result.unwrap_or_else(|error| panic!("{}", error.describe()))
}

fn pi_at(point: Point2) -> AlignmentPi {
    AlignmentPi {
        point,
        ..AlignmentPi::default()
    }
}

fn straight(from: Point2, to: Point2) -> SolvedAlignment {
    let definition = HorizontalAlignment {
        pis: vec![pi_at(from), pi_at(to)],
        ..HorizontalAlignment::default()
    };
    must(solve_alignment(&definition))
}

fn plane_surface(
    plane: Plane,
    size: f64,
    interior: usize,
    seed: u64,
    x0: f64,
    y0: f64,
) -> TinSurface {
    let mut random = Random::new(seed);
    build_from_points(plane_points_at(&plane, size, interior, &mut random, x0, y0))
}

/// A wedge whose cross section grows in both width and depth along the
/// alignment, so its area is quadratic in station: the ground z = 0 over
/// [-10, 110] x [-20, 20], and over it the single triangle (0, 0), (100, -10),
/// (100, 10) of the plane z = 0.1 x. At station s the wedge is s / 5 wide and
/// 0.1 s deep: A(s) = s^2 / 50.
struct Wedge {
    ground: TinSurface,
    top: TinSurface,
}

fn wedge() -> Wedge {
    Wedge {
        ground: build_from_points(vec![
            p3(-10.0, -20.0, 0.0),
            p3(110.0, -20.0, 0.0),
            p3(110.0, 20.0, 0.0),
            p3(-10.0, 20.0, 0.0),
        ]),
        top: build_from_points(vec![
            p3(0.0, 0.0, 0.0),
            p3(100.0, -10.0, 10.0),
            p3(100.0, 10.0, 10.0),
        ]),
    }
}

fn same_bits(a: f64, b: f64) -> bool {
    a.to_bits() == b.to_bits()
}

/// The square [x0, x0 + size] x [y0, y0 + size] flat at height z: two
/// triangles.
fn slab(x0: f64, y0: f64, size: f64, z: f64) -> TinSurface {
    build_from_points(vec![
        p3(x0, y0, z),
        p3(x0 + size, y0, z),
        p3(x0 + size, y0 + size, z),
        p3(x0, y0 + size, z),
    ])
}

/// An alignment through the points, each with a radius (0: a corner).
fn through(pis: &[(Point2, f64)]) -> SolvedAlignment {
    let definition = HorizontalAlignment {
        pis: pis
            .iter()
            .map(|&(point, radius)| AlignmentPi {
                point,
                radius,
                ..AlignmentPi::default()
            })
            .collect(),
        ..HorizontalAlignment::default()
    };
    must(solve_alignment(&definition))
}

fn volumes(
    existing: &TinSurface,
    design: &TinSurface,
    alignment: &SolvedAlignment,
    options: &AlignmentVolumeOptions,
) -> Result<AlignmentVolumes> {
    volumes_along_alignment(existing, design, alignment, options, None)
}

/// Fifty metres of tangent east from (0, 0), a quarter circle of radius 50
/// turning left about the centre (50, 50), fifty metres of tangent north to
/// (100, 100): the PI is (100, 0), a 90 degree deflection, and the tangents
/// are 50 long on each side of the curve's own 25 pi = 78.540.
fn quarter_circle_left() -> SolvedAlignment {
    through(&[
        (p2(0.0, 0.0), 0.0),
        (p2(100.0, 0.0), 50.0),
        (p2(100.0, 100.0), 0.0),
    ])
}

/// The same way back, from (100, 100) south and then west: it turns right
/// about the same centre, so its inside is on the right.
fn quarter_circle_right() -> SolvedAlignment {
    through(&[
        (p2(100.0, 100.0), 0.0),
        (p2(100.0, 0.0), 50.0),
        (p2(0.0, 0.0), 0.0),
    ])
}

fn strip_reach(left: f64, right: f64, interval: f64, curvature: bool) -> AlignmentVolumeOptions {
    AlignmentVolumeOptions {
        interval,
        left: Some(left),
        right: Some(right),
        curvature,
        ..AlignmentVolumeOptions::default()
    }
}

/// East 100 m, north 15 m, west 100 m: a U-turn of two corners, whose legs are
/// 15 m apart. With a reach under 7.5 each way the strips of the two legs clear
/// one another.
fn u_turn() -> SolvedAlignment {
    through(&[
        (p2(0.0, 0.0), 0.0),
        (p2(100.0, 0.0), 0.0),
        (p2(100.0, 15.0), 0.0),
        (p2(0.0, 15.0), 0.0),
    ])
}

// ---- along a segment ----------------------------------------------------------------------

mod segment_profiler {
    use super::*;

    #[test]
    fn a_plane_read_along_a_segment_is_the_plane_on_every_piece_and_the_pieces_meet_exactly() {
        let plane = Plane::new(0.3, -0.2, 50.0);
        let surface = plane_surface(plane, 100.0, 400, 3, 0.0, 0.0);
        let profiler = SegmentProfiler::new(&surface);
        let from = p2(3.0, 7.0);
        let to = p2(96.0, 81.0);
        let pieces = profiler.along(from, to);
        assert!(pieces.len() > 5);
        assert_eq!(pieces[0].t0, 0.0);
        assert_eq!(pieces[pieces.len() - 1].t1, 1.0);
        let at = |t: f64| plane.at(from.x + t * (to.x - from.x), from.y + t * (to.y - from.y));
        for (i, piece) in pieces.iter().enumerate() {
            assert!(piece.t0 < piece.t1);
            // Heights to 80, read off planes of a plane: a few ulps.
            near(piece.z0, at(piece.t0), 1e-11);
            near(piece.z1, at(piece.t1), 1e-11);
            if let Some(next) = pieces.get(i + 1) {
                // The crossing of an edge is computed from the edge, so the
                // pieces either side of it meet at the same bits.
                assert!(same_bits(piece.t1, next.t0), "{i}");
                assert!(same_bits(piece.z1, next.z0), "{i}");
            }
        }
    }

    #[test]
    fn off_the_surface_there_are_no_pieces_and_partly_on_it_only_the_part_on() {
        let surface = plane_surface(Plane::new(0.0, 0.0, 1.0), 10.0, 10, 4, 0.0, 0.0);
        let profiler = SegmentProfiler::new(&surface);
        assert!(profiler.along(p2(20.0, 0.0), p2(30.0, 5.0)).is_empty());
        assert!(profiler.along(p2(5.0, 5.0), p2(5.0, 5.0)).is_empty()); // no length
        // From x = -10 to x = 10 along y = 5: on the surface from the middle on.
        let half = profiler.along(p2(-10.0, 5.0), p2(10.0, 5.0));
        assert!(!half.is_empty());
        near(half[0].t0, 0.5, 1e-15);
        assert_eq!(half[half.len() - 1].t1, 1.0);
    }

    // ---- sections that lie on a surface's boundary ---------------------------------------------

    #[test]
    fn a_segment_along_a_boundary_edge_is_on_the_surface_whole_to_within_the_geometric_tolerance() {
        // The square [0, 100]^2; a segment from x = -10 to x = 110 has 100 of
        // its 120 on it.
        let surface = slab(0.0, 0.0, 100.0, 5.0);
        let profiler = SegmentProfiler::new(&surface);
        let covered = |from: Point2, to: Point2| {
            let mut sum = 0.0;
            for piece in profiler.along(from, to) {
                sum += piece.t1 - piece.t0;
            }
            sum
        };
        // Along the south edge y = 0 with the ends 5e-15 either side of it, as
        // the rounding of a bearing leaves them (cos(pi/2) = 6e-17 times a
        // reach of 141 m is 9e-15): the tilt crosses the edge at the middle,
        // and half of the segment used to be outside.
        near(
            covered(p2(-10.0, 5e-15), p2(110.0, -5e-15)),
            100.0 / 120.0,
            1e-12,
        );
        near(
            covered(p2(-10.0, -5e-15), p2(110.0, 5e-15)),
            100.0 / 120.0,
            1e-12,
        );
        // The same along the north edge y = 100, run the other way.
        near(
            covered(p2(110.0, 100.0 + 5e-14), p2(-10.0, 100.0 - 5e-14)),
            100.0 / 120.0,
            1e-12,
        );
        // And the two edges that run up the page.
        near(
            covered(p2(5e-15, -10.0), p2(-5e-15, 110.0)),
            100.0 / 120.0,
            1e-12,
        );
        near(
            covered(p2(100.0 - 5e-14, 110.0), p2(100.0 + 5e-14, -10.0)),
            100.0 / 120.0,
            1e-12,
        );
        // The tolerance is GEOMETRIC, 1e-7: 5e-8 outside the edge is the edge,
        // 5e-7 outside is off the surface, and 5e-7 inside is on it, as it
        // always was. Within it, not exactly: the covered piece may run past
        // each end of the edge by up to GEOMETRIC, so 100 m +- 2e-7 of the
        // segment's 120 is 100/120 +- 1.7e-9.
        near(
            covered(p2(-10.0, -5e-8), p2(110.0, -5e-8)),
            100.0 / 120.0,
            2e-7 / 120.0,
        );
        assert_eq!(covered(p2(-10.0, -5e-7), p2(110.0, -5e-7)), 0.0);
        near(
            covered(p2(-10.0, 5e-7), p2(110.0, 5e-7)),
            100.0 / 120.0,
            1e-12,
        );
        // The neighbours of the case: a short segment wholly on the edge is
        // wholly on the surface, and one that only meets the corner has no
        // length on it.
        near(covered(p2(40.0, 5e-15), p2(60.0, -5e-15)), 1.0, 1e-12);
        near(covered(p2(-10.0, -10.0), p2(0.0, 0.0)), 0.0, 1e-12);
    }
}

// ---- one section ---------------------------------------------------------------------------

mod signed_trapezoid {
    use super::*;

    #[test]
    fn splits_at_the_zero_where_the_sides_differ_in_sign() {
        // Width 2: sides 1 and 3 make (1 + 3) / 2 x 2 = 4 positive; -1 and -3
        // as much negative; 2 and -2 cross in the middle, two triangles of
        // 2 x 1 / 2 = 1 each.
        double_eq(signed_trapezoid(1.0, 3.0, 2.0).positive, 4.0);
        assert_eq!(signed_trapezoid(1.0, 3.0, 2.0).negative, 0.0);
        double_eq(signed_trapezoid(-1.0, -3.0, 2.0).negative, 4.0);
        double_eq(signed_trapezoid(2.0, -2.0, 2.0).positive, 1.0);
        double_eq(signed_trapezoid(2.0, -2.0, 2.0).negative, 1.0);
        assert_eq!(signed_trapezoid(2.0, 2.0, 0.0).positive, 0.0);
    }

    // ---- the first moments of a section ----------------------------------------------------------

    #[test]
    fn moments_are_the_integral_of_depth_times_offset_and_split_where_the_sides_differ_in_sign() {
        // A strip 2 wide whose first side is 5 from the line, the offset
        // falling to 3 at its far side, so o(u) = 5 - u. Depth 1 to 3,
        // d(u) = 1 + u:
        //   integral of (1 + u)(5 - u) = (5 + 4u - u^2) over [0, 2]
        //     = 10 + 8 - 8/3 = 46/3.
        near(
            signed_trapezoid_moments(1.0, 3.0, 2.0, 5.0).positive,
            46.0 / 3.0,
            1e-12,
        );
        assert_eq!(signed_trapezoid_moments(1.0, 3.0, 2.0, 5.0).negative, 0.0);
        // The same depths below zero are the same moment in the negative part,
        // the depth taken as a magnitude.
        near(
            signed_trapezoid_moments(-1.0, -3.0, 2.0, 5.0).negative,
            46.0 / 3.0,
            1e-12,
        );
        assert_eq!(signed_trapezoid_moments(-1.0, -3.0, 2.0, 5.0).positive, 0.0);
        // Depth 2 to -2 crossing in the middle: the positive triangle is over u
        // in [0, 1] with d = 2 - 2u:
        //   (2 - 2u)(5 - u) = 10 - 12u + 2u^2, integrated over [0, 1]:
        //   10 - 6 + 2/3 = 14/3;
        // the negative one over [1, 2] with the magnitude 2u - 2:
        //   (2u - 2)(5 - u) = -2u^2 + 12u - 10, integrated: [-2u^3/3 + 6u^2 - 10u]
        //   from 1 to 2 = (-16/3 + 24 - 20) - (-2/3 + 6 - 10) = -4/3 + 14/3 = 10/3.
        near(
            signed_trapezoid_moments(2.0, -2.0, 2.0, 5.0).positive,
            14.0 / 3.0,
            1e-12,
        );
        near(
            signed_trapezoid_moments(2.0, -2.0, 2.0, 5.0).negative,
            10.0 / 3.0,
            1e-12,
        );
        // Mirrored, the parts swap sides: the negative one is first.
        near(
            signed_trapezoid_moments(-2.0, 2.0, 2.0, 5.0).negative,
            14.0 / 3.0,
            1e-12,
        );
        near(
            signed_trapezoid_moments(-2.0, 2.0, 2.0, 5.0).positive,
            10.0 / 3.0,
            1e-12,
        );
        // Each moment over its area is a centroid between the part's sides: the
        // positive triangle's is 1/3 of its base from the tall side, 5 - 1/3.
        let areas = signed_trapezoid(2.0, -2.0, 2.0);
        near(
            signed_trapezoid_moments(2.0, -2.0, 2.0, 5.0).positive / areas.positive,
            5.0 - 1.0 / 3.0,
            1e-12,
        );
        near(
            signed_trapezoid_moments(2.0, -2.0, 2.0, 5.0).negative / areas.negative,
            5.0 - 5.0 / 3.0,
            1e-12,
        );
    }

    #[test]
    fn a_strip_on_the_other_side_of_the_line_has_a_negative_moment_and_a_centred_one_none() {
        // Depth 2 all across a strip 2 wide that starts at offset -1 and falls
        // to -3: integral of 2 (-1 - u) over [0, 2] = 2 (-2 - 2) = -8.
        near(
            signed_trapezoid_moments(2.0, 2.0, 2.0, -1.0).positive,
            -8.0,
            1e-12,
        );
        // A strip centred on the line (first side at +1, last at -1) has no
        // moment whatever it is deep, and neither has any nothing.
        near(
            signed_trapezoid_moments(1.0, 1.0, 2.0, 1.0).positive,
            0.0,
            1e-12,
        );
        assert_eq!(signed_trapezoid_moments(0.0, 0.0, 2.0, 5.0).positive, 0.0);
        assert_eq!(signed_trapezoid_moments(0.0, 0.0, 2.0, 5.0).negative, 0.0);
        // No width, no moment; a width that is not a number likewise.
        assert_eq!(signed_trapezoid_moments(2.0, 2.0, 0.0, 5.0).positive, 0.0);
        assert_eq!(signed_trapezoid_moments(2.0, 2.0, -1.0, 5.0).positive, 0.0);
        assert_eq!(
            signed_trapezoid_moments(2.0, 2.0, f64::NAN, 5.0).positive,
            0.0
        );
    }
}

mod section_areas {
    use super::*;

    fn along(profiler: &SegmentProfiler<'_>, from: Point2, to: Point2) -> Vec<SegmentPiece> {
        profiler.along(from, to)
    }

    #[test]
    fn two_parallel_planes_one_metre_apart_across_thirty_metres_are_thirty_square_metres() {
        let base = plane_surface(Plane::new(0.02, 0.01, 10.0), 100.0, 150, 5, 0.0, 0.0);
        let top = plane_surface(Plane::new(0.02, 0.01, 11.0), 100.0, 120, 6, 0.0, 0.0);
        let under = SegmentProfiler::new(&base);
        let over = SegmentProfiler::new(&top);
        let from = p2(50.0, 20.0);
        let to = p2(50.0, 50.0);
        let areas = section_areas(&along(&under, from, to), &along(&over, from, to), 30.0, 0.0);
        near(areas.fill, 30.0, 1e-11);
        near(areas.cut, 0.0, 1e-12);
        near(areas.width, 30.0, 1e-12);
    }

    #[test]
    fn where_only_one_surface_has_a_height_nothing_is_measured() {
        // The top covers x up to 10 of a section 20 long: half of it.
        let base = plane_surface(Plane::new(0.0, 0.0, 0.0), 20.0, 10, 7, 0.0, 0.0);
        let top = plane_surface(Plane::new(0.0, 0.0, 2.0), 10.0, 10, 8, 0.0, 0.0);
        let under = SegmentProfiler::new(&base);
        let over = SegmentProfiler::new(&top);
        let areas = section_areas(
            &along(&under, p2(0.0, 5.0), p2(20.0, 5.0)),
            &along(&over, p2(0.0, 5.0), p2(20.0, 5.0)),
            20.0,
            0.0,
        );
        near(areas.width, 10.0, 1e-12);
        near(areas.fill, 20.0, 1e-11);
    }

    #[test]
    fn are_their_first_moments_about_the_line_the_offset_is_measured_from() {
        // A section 20 long from (0, 10) to (0, -10), so the offset of a point
        // is its y when the first end is given 10 from the line. The base is
        // flat at 0 and the design z = 0.1 y + 1: 2 deep at y = 10, 0 at
        // y = -10. A triangle of base 20 and height 2: area 20, and its
        // centroid 1/3 of the base from the deep end, y = 10 - 20/3 = 10/3, so
        // the moment is 20 x 10/3 = 200/3. Also by the integral:
        // (0.1 y + 1) y over [-10, 10] = 0.1 x 2000/3 = 200/3.
        let base = plane_surface(Plane::new(0.0, 0.0, 0.0), 100.0, 60, 31, -50.0, -50.0);
        let top = plane_surface(Plane::new(0.0, 0.1, 1.0), 100.0, 60, 32, -50.0, -50.0);
        let under = SegmentProfiler::new(&base);
        let over = SegmentProfiler::new(&top);
        let from = p2(0.0, 10.0);
        let to = p2(0.0, -10.0);
        let areas = section_areas(
            &along(&under, from, to),
            &along(&over, from, to),
            20.0,
            10.0,
        );
        near(areas.fill, 20.0, 1e-10);
        near(areas.fill_moment, 200.0 / 3.0, 1e-9);
        // The design is 0 deep at the far end, where rounding may leave -1e-16.
        near(areas.cut, 0.0, 1e-12);
        near(areas.cut_moment, 0.0, 1e-11);
        // With the offset measured from the first end itself (the default) the
        // offset is -x along the section, and the centroid is 20/3 from the
        // deep end, which is the first: the moment is 20 x (-20/3) = -400/3. By
        // the integral: (2 - 0.1 x)(-x) over [0, 20] = -(400 - 0.1 x 8000/3) =
        // -(400 - 800/3) = -400/3.
        let from_the_end =
            section_areas(&along(&under, from, to), &along(&over, from, to), 20.0, 0.0);
        near(from_the_end.fill_moment, -400.0 / 3.0, 1e-9);
        // Swapped, the design is the lower: all cut, the same magnitudes in the
        // cut.
        let swapped = section_areas(
            &along(&over, from, to),
            &along(&under, from, to),
            20.0,
            10.0,
        );
        near(swapped.cut, 20.0, 1e-10);
        near(swapped.cut_moment, 200.0 / 3.0, 1e-9);
        near(swapped.fill, 0.0, 1e-12);
        near(swapped.fill_moment, 0.0, 1e-11);
    }

    #[test]
    fn a_section_where_the_surfaces_cross_has_a_cut_moment_on_one_side_and_a_fill_moment_on_the_other()
     {
        // The same section against z = 0.2 y: fill for y > 0 (depth 0.2 y, area
        // 0.5 x 10 x 2 = 10, moment the integral of 0.2 y^2 over [0, 10] =
        // 200/3) and cut for y < 0 (area 10 again; the magnitude 0.2 |y| times
        // the offset y, negative: -200/3).
        let base = plane_surface(Plane::new(0.0, 0.0, 0.0), 100.0, 60, 33, -50.0, -50.0);
        let top = plane_surface(Plane::new(0.0, 0.2, 0.0), 100.0, 60, 34, -50.0, -50.0);
        let under = SegmentProfiler::new(&base);
        let over = SegmentProfiler::new(&top);
        let from = p2(0.0, 10.0);
        let to = p2(0.0, -10.0);
        let areas = section_areas(
            &along(&under, from, to),
            &along(&over, from, to),
            20.0,
            10.0,
        );
        near(areas.fill, 10.0, 1e-10);
        near(areas.cut, 10.0, 1e-10);
        near(areas.fill_moment, 200.0 / 3.0, 1e-9);
        near(areas.cut_moment, -200.0 / 3.0, 1e-9);
    }
}

// ---- along an alignment ------------------------------------------------------------------------

mod volumes_along_alignment {
    use super::*;

    #[test]
    fn on_a_wedge_the_prismoidal_formula_is_exact_and_the_end_area_overstates_it() {
        // A(s) = s^2 / 50 from 0 to 100 (see wedge()). Exactly, V = 100^3 /
        // 150 = 6666.667 m^3, which the exact overlay also gives: 0.1 x the
        // triangle's area 1000 x its centroid's x 200 / 3. On sections 10
        // apart:
        //   prismoidal: Simpson's rule on a quadratic, exact, 20 000 / 3;
        //   average end area: 10 sum (A_i + A_i+1) / 2 = 10 sum (i^2 + (i+1)^2)
        //   for i = 0..9 = 10 (285 + 385) = 6700, the trapezoid rule's
        //   (b - a) h^2 A'' / 12 = 100 x 100 x (1/25) / 12 = 33.333 over.
        let w = wedge();
        let alignment = straight(p2(0.0, 0.0), p2(100.0, 0.0));
        let options = AlignmentVolumeOptions {
            interval: 10.0,
            left: Some(20.0),
            right: Some(20.0),
            prismoidal: true,
            ..AlignmentVolumeOptions::default()
        };
        let result = must(volumes(&w.ground, &w.top, &alignment, &options));
        assert_eq!(result.sections.len(), 11);
        assert_eq!(result.panels.len(), 10);
        for (i, section) in result.sections.iter().enumerate() {
            let s = 10.0 * i as f64;
            near(section.station, s, 1e-12);
            assert!((section.fill_area - s * s / 50.0).abs() <= 1e-10, "{s}");
            assert!((section.width - s / 5.0).abs() <= 1e-12, "{s}");
            assert_eq!(section.cut_area, 0.0);
        }
        near(result.end_area_fill, 6700.0, 1e-9);
        near(result.prismoidal_fill, 20000.0 / 3.0, 1e-9);
        assert_eq!(result.end_area_cut, 0.0);
        // The middle section of the first panel, at 5: 25 / 50.
        let middle = result.panels[0].middle.expect("a middle section");
        near(middle.fill_area, 0.5, 1e-12);

        let exact = must(compare_surfaces(&w.ground, &w.top, None));
        near(exact.fill, 20000.0 / 3.0, 1e-9);
        near(result.prismoidal_fill, exact.fill, 1e-9);
    }

    #[test]
    fn two_parallel_planes_give_the_slab_by_either_method() {
        // 1 m apart, sections 20 m across, 100 m: 2000 m^3, and with a constant
        // area the two formulas agree exactly.
        let base = plane_surface(Plane::new(0.02, -0.01, 30.0), 140.0, 200, 9, -20.0, -70.0);
        let top = plane_surface(Plane::new(0.02, -0.01, 31.0), 140.0, 180, 10, -20.0, -70.0);
        let alignment = straight(p2(0.0, 0.0), p2(100.0, 0.0));
        let options = AlignmentVolumeOptions {
            interval: 25.0,
            left: Some(10.0),
            right: Some(10.0),
            prismoidal: true,
            ..AlignmentVolumeOptions::default()
        };
        let result = must(volumes(&base, &top, &alignment, &options));
        assert_eq!(result.sections.len(), 5);
        near(result.end_area_fill, 2000.0, 1e-9);
        near(result.prismoidal_fill, 2000.0, 1e-9);
        near(result.end_area_cut, 0.0, 1e-12);
    }

    #[test]
    fn sections_are_cut_at_the_alignments_key_stations_too() {
        // A curve of radius 30 at the middle PI: its tangent points are key
        // stations, and every one of them inside the range has a section,
        // marked key, besides the round tens.
        let definition = HorizontalAlignment {
            pis: vec![
                pi_at(p2(0.0, 0.0)),
                AlignmentPi {
                    point: p2(60.0, 0.0),
                    radius: 30.0,
                    ..AlignmentPi::default()
                },
                pi_at(p2(60.0, 60.0)),
            ],
            ..HorizontalAlignment::default()
        };
        let alignment = must(solve_alignment(&definition));
        let base = plane_surface(Plane::new(0.0, 0.0, 0.0), 200.0, 20, 11, -50.0, -50.0);
        let top = plane_surface(Plane::new(0.0, 0.0, 1.0), 200.0, 20, 12, -50.0, -50.0);
        let options = AlignmentVolumeOptions {
            left: Some(5.0),
            right: Some(5.0),
            ..AlignmentVolumeOptions::default()
        };
        let result = must(volumes(&base, &top, &alignment, &options));
        for key in alignment.key_stations() {
            let found = result
                .sections
                .iter()
                .any(|section| (section.station - key).abs() < 1e-9 && section.key);
            assert!(found, "key station {key}");
        }
        // A slab 1 deep and 10 wide all along the centreline: 10 m^2 on every
        // section, and on a curve's inside and outside the sections still have
        // that area, so end areas give 10 x the length.
        for section in &result.sections {
            assert!(
                (section.fill_area - 10.0).abs() <= 1e-9,
                "{}",
                section.station
            );
        }
        near(result.end_area_fill, 10.0 * alignment.length(), 1e-7);
    }

    #[test]
    fn a_from_and_to_range_is_measured_alone() {
        let w = wedge();
        let alignment = straight(p2(0.0, 0.0), p2(100.0, 0.0));
        let options = AlignmentVolumeOptions {
            interval: 10.0,
            from: Some(50.0),
            to: Some(100.0),
            left: Some(20.0),
            right: Some(20.0),
            prismoidal: true,
            ..AlignmentVolumeOptions::default()
        };
        let result = must(volumes(&w.ground, &w.top, &alignment, &options));
        // The integral of s^2 / 50 from 50 to 100: (100^3 - 50^3) / 150 =
        // 5833.333.
        near(result.prismoidal_fill, (1e6 - 125000.0) / 150.0, 1e-9);
        near(result.sections[0].station, 50.0, 1e-12);
        assert!(result.sections[0].key);
    }

    #[test]
    fn refuses_what_cannot_be_measured() {
        let w = wedge();
        let alignment = straight(p2(0.0, 0.0), p2(100.0, 0.0));
        let refused = |options: AlignmentVolumeOptions, code: ErrorCode| {
            let error =
                volumes(&w.ground, &w.top, &alignment, &options).expect_err("it is refused");
            assert_eq!(error.code, code, "{}", error.describe());
        };
        refused(
            AlignmentVolumeOptions {
                interval: 0.0,
                ..AlignmentVolumeOptions::default()
            },
            ErrorCode::InvalidArgument,
        );
        refused(
            AlignmentVolumeOptions {
                from: Some(80.0),
                to: Some(20.0),
                ..AlignmentVolumeOptions::default()
            },
            ErrorCode::InvalidArgument,
        );
        refused(
            AlignmentVolumeOptions {
                to: Some(150.0),
                ..AlignmentVolumeOptions::default()
            },
            ErrorCode::InvalidArgument,
        );
        refused(
            AlignmentVolumeOptions {
                left: Some(-1.0),
                ..AlignmentVolumeOptions::default()
            },
            ErrorCode::InvalidArgument,
        );
        refused(
            AlignmentVolumeOptions {
                left: Some(0.0),
                right: Some(0.0),
                ..AlignmentVolumeOptions::default()
            },
            ErrorCode::InvalidArgument,
        );
        refused(
            AlignmentVolumeOptions {
                interval: 1e-4, // 100 m at 0.1 mm: a million sections
                ..AlignmentVolumeOptions::default()
            },
            ErrorCode::InvalidArgument,
        );

        let far = plane_surface(Plane::new(0.0, 0.0, 0.0), 10.0, 5, 13, 500.0, 500.0);
        let apart = volumes(
            &w.ground,
            &far,
            &alignment,
            &AlignmentVolumeOptions::default(),
        )
        .expect_err("no ground in common");
        assert_eq!(apart.code, ErrorCode::InvalidGeometry);
        let empty = volumes(
            &w.ground,
            &TinSurface::new(),
            &alignment,
            &AlignmentVolumeOptions::default(),
        )
        .expect_err("an empty surface");
        assert_eq!(empty.code, ErrorCode::InvalidArgument);
    }

    #[test]
    fn gives_the_same_bits_on_one_thread_as_on_many() {
        let mut random = Random::new(91);
        let mut rough = TinInput::default();
        for _ in 0..2000 {
            let x = random.real(-50.0, 150.0);
            let y = random.real(-50.0, 50.0);
            let z = random.real(0.0, 4.0);
            rough.points.push(p3(x, y, z));
        }
        let base = build(&rough).surface;
        let top = plane_surface(Plane::new(0.0, 0.0, 2.0), 200.0, 50, 92, -50.0, -100.0);
        let alignment = straight(p2(0.0, 0.0), p2(100.0, 0.0));
        let options = AlignmentVolumeOptions {
            interval: 2.0,
            left: Some(30.0),
            right: Some(30.0),
            prismoidal: true,
            ..AlignmentVolumeOptions::default()
        };
        let serial = TaskPool::new(0);
        let one = must(volumes_along_alignment(
            &base,
            &top,
            &alignment,
            &options,
            Some(&serial),
        ));
        for workers in [1, 4] {
            let pool = TaskPool::new(workers);
            let many = must(volumes_along_alignment(
                &base,
                &top,
                &alignment,
                &options,
                Some(&pool),
            ));
            assert!(same_bits(one.end_area_cut, many.end_area_cut), "{workers}");
            assert!(
                same_bits(one.end_area_fill, many.end_area_fill),
                "{workers}"
            );
            assert!(
                same_bits(one.prismoidal_cut, many.prismoidal_cut),
                "{workers}"
            );
            assert!(
                same_bits(one.prismoidal_fill, many.prismoidal_fill),
                "{workers}"
            );
        }
    }

    #[test]
    fn a_slab_measured_along_any_cardinal_alignment_is_whole_where_its_end_sections_lie_on_its_boundary()
     {
        // A 1 m slab over [0, 100]^2 holds 100 x 100 x 1 = 10 000 m3, and every
        // section across it, 100 m wide, is 100 m2: the first and last lie ON
        // the slab's edges. The default reach (the diagonal, 141.4 m) takes
        // each section out past the slab on both sides. East has no tilt
        // (sin 0 = 0); north, south and west used to lose half of an end
        // section, 500 m3 in all.
        let base = slab(0.0, 0.0, 100.0, 10.0);
        let top = slab(0.0, 0.0, 100.0, 11.0);
        let exact = must(compare_surfaces(&base, &top, None));
        near(exact.fill, 10000.0, 1e-9);
        for (name, from, to) in [
            ("north", p2(50.0, 0.0), p2(50.0, 100.0)),
            ("south", p2(50.0, 100.0), p2(50.0, 0.0)),
            ("east", p2(0.0, 50.0), p2(100.0, 50.0)),
            ("west", p2(100.0, 50.0), p2(0.0, 50.0)),
        ] {
            let options = AlignmentVolumeOptions {
                interval: 10.0,
                ..AlignmentVolumeOptions::default()
            };
            let result = volumes(&base, &top, &straight(from, to), &options)
                .unwrap_or_else(|error| panic!("{name}: {}", error.describe()));
            assert_eq!(result.sections.len(), 11, "{name}");
            for section in &result.sections {
                assert!(
                    (section.width - 100.0).abs() <= 1e-9,
                    "{name} at {}",
                    section.station
                );
                assert!(
                    (section.fill_area - 100.0).abs() <= 1e-9,
                    "{name} at {}",
                    section.station
                );
                assert_eq!(section.cut_area, 0.0, "{name}");
            }
            assert!((result.end_area_fill - 10000.0).abs() <= 1e-6, "{name}");
            assert_eq!(result.end_area_cut, 0.0, "{name}");
            assert!((result.end_area_fill - exact.fill).abs() <= 1e-6, "{name}");
        }
    }

    #[test]
    fn a_slab_whose_ends_are_cut_by_the_alignment_itself_is_whole_at_every_bearing() {
        // The slab a corridor leaves: 80 m along an alignment of any bearing
        // and 50 across, its four corners placed by the alignment's own
        // point_at_station_offset as a corridor's are, so its end edges are the
        // first and last sections to rounding and not to the bit. 1 m deep:
        // 80 x 50 x 1 = 4000 m3, by sections of 50 m2 every 20 m. Every
        // fifteenth degree, east, north, west and south among them.
        for degrees in (0..360).step_by(15) {
            let angle = f64::from(degrees) * DEG_TO_RAD;
            let start = p2(30.0, 40.0);
            let alignment = straight(
                start,
                p2(start.x + 80.0 * angle.cos(), start.y + 80.0 * angle.sin()),
            );
            let length = alignment.length();
            let corner = |station: f64, offset: f64, z: f64| -> Point3 {
                let at = alignment
                    .point_at_station_offset(station, offset)
                    .expect("on the alignment");
                p3(at.x, at.y, z)
            };
            let slab_at = |z: f64| {
                build_from_points(vec![
                    corner(0.0, 25.0, z),
                    corner(0.0, -25.0, z),
                    corner(length, -25.0, z),
                    corner(length, 25.0, z),
                ])
            };
            let options = AlignmentVolumeOptions {
                interval: 20.0,
                left: Some(40.0),
                right: Some(40.0),
                ..AlignmentVolumeOptions::default()
            };
            let result = volumes(&slab_at(10.0), &slab_at(11.0), &alignment, &options)
                .unwrap_or_else(|error| panic!("{degrees}: {}", error.describe()));
            assert_eq!(result.sections.len(), 5, "{degrees}");
            for section in &result.sections {
                assert!(
                    (section.width - 50.0).abs() <= 1e-9,
                    "{degrees} at {}",
                    section.station
                );
                assert!(
                    (section.fill_area - 50.0).abs() <= 1e-9,
                    "{degrees} at {}",
                    section.station
                );
            }
            assert!(
                (result.end_area_fill - 50.0 * length).abs() <= 1e-6,
                "{degrees}"
            );
        }
    }

    #[test]
    fn a_slab_at_survey_coordinates_is_whole_on_the_cardinal_bearings_too() {
        // UTM-sized coordinates, where the tilt of a section is below half an
        // ulp (9e-10 at 6e6) and the tolerance must neither lose ground nor
        // invent it: the slab [500000, 500100] x [6000000, 6000100] is
        // 10 000 m3.
        let base = slab(500000.0, 6000000.0, 100.0, 10.0);
        let top = slab(500000.0, 6000000.0, 100.0, 11.0);
        for (name, from, to) in [
            ("north", p2(500050.0, 6000000.0), p2(500050.0, 6000100.0)),
            ("south", p2(500050.0, 6000100.0), p2(500050.0, 6000000.0)),
            ("east", p2(500000.0, 6000050.0), p2(500100.0, 6000050.0)),
            ("west", p2(500100.0, 6000050.0), p2(500000.0, 6000050.0)),
        ] {
            let result = volumes(
                &base,
                &top,
                &straight(from, to),
                &AlignmentVolumeOptions::default(),
            )
            .unwrap_or_else(|error| panic!("{name}: {}", error.describe()));
            assert!((result.end_area_fill - 10000.0).abs() <= 1e-5, "{name}");
            for section in &result.sections {
                assert!(
                    (section.width - 100.0).abs() <= 1e-6,
                    "{name} at {}",
                    section.station
                );
            }
        }
    }

    // ---- sections that cross ---------------------------------------------------------------------

    #[test]
    fn sections_across_a_curve_deeper_than_its_radius_are_refused_not_counted_twice() {
        // A 1 m slab over [0, 200]^2 is 40 000 m3 in all. The alignment
        // (20, 20) to (180, 20) to (180, 180) with a curve of radius 40 at the
        // corner has its tangent points 40 from the corner:
        // 120 + (pi/2) 40 + 120 = 240 + 20 pi = 302.832 m. With the default
        // reach (the diagonal, 282.8 m) the sections of the curve all pass
        // through its centre, and the two tangents' sections sweep the same
        // 120 x 120 block: the sum was 62 101 m3, more than there is.
        let alignment = through(&[
            (p2(20.0, 20.0), 0.0),
            (p2(180.0, 20.0), 40.0),
            (p2(180.0, 180.0), 0.0),
        ]);
        near(alignment.length(), 240.0 + 20.0 * PI, 1e-9);
        let base = slab(0.0, 0.0, 200.0, 10.0);
        let top = slab(0.0, 0.0, 200.0, 11.0);

        let mut options = AlignmentVolumeOptions {
            interval: 10.0,
            ..AlignmentVolumeOptions::default()
        };
        let refused = volumes(&base, &top, &alignment, &options).expect_err("crossing sections");
        assert_eq!(refused.code, ErrorCode::InvalidArgument);
        assert!(
            refused.message.contains("cross each other"),
            "{}",
            refused.message
        );
        // The reach that clears every crossing is the radius, to the
        // millimetre.
        assert!(
            refused.message.contains("at most 40.000 m"),
            "{}",
            refused.message
        );
        // The same pair and the same reach whatever the thread count.
        let serial = TaskPool::new(0);
        let many = TaskPool::new(4);
        let on_one = volumes_along_alignment(&base, &top, &alignment, &options, Some(&serial))
            .expect_err("crossing sections");
        let on_many = volumes_along_alignment(&base, &top, &alignment, &options, Some(&many))
            .expect_err("crossing sections");
        assert_eq!(on_one.message, refused.message);
        assert_eq!(on_many.message, refused.message);

        // A reach one metre past the radius is refused as the default is.
        options.left = Some(41.0);
        options.right = Some(41.0);
        assert!(volumes(&base, &top, &alignment, &options).is_err());
        // A reach of 15 either side is a band 30 m wide along the centreline:
        // 30 x 302.832 = 7200 + 600 pi = 9084.956 m3, the area of a band round
        // a curve being its width times its centreline's length (Pappus; the
        // eccentricities of the two sides cancel), whatever the interval.
        options.left = Some(15.0);
        options.right = Some(15.0);
        let band = must(volumes(&base, &top, &alignment, &options));
        near(band.end_area_fill, 7200.0 + 600.0 * PI, 1e-6);
        assert!(band.end_area_fill < 40000.0);
        for section in &band.sections {
            assert!(
                (section.fill_area - 30.0).abs() <= 1e-9,
                "{}",
                section.station
            );
        }
        // A reach of exactly the radius on the inside ends every section of
        // the curve on its centre: touching, not crossing, and allowed.
        options.left = Some(40.0);
        options.right = Some(10.0);
        assert!(volumes(&base, &top, &alignment, &options).is_ok());
    }

    #[test]
    fn two_straight_legs_at_a_right_angle_are_refused_with_the_default_reach() {
        // No curve at all: legs along y = 0 and x = 100, a corner with no
        // radius. The first leg's sections, 283 m long, cross the second leg's
        // wherever they meet, a long way from the corner.
        let alignment = through(&[
            (p2(0.0, 0.0), 0.0),
            (p2(100.0, 0.0), 0.0),
            (p2(100.0, 50.0), 0.0),
        ]);
        let base = slab(-200.0, -200.0, 500.0, 0.0);
        let top = slab(-200.0, -200.0, 500.0, 1.0);
        let refused = volumes(&base, &top, &alignment, &AlignmentVolumeOptions::default())
            .expect_err("crossing sections");
        assert_eq!(refused.code, ErrorCode::InvalidArgument);
    }

    #[test]
    fn a_corner_allows_a_reach_up_to_the_interval_over_the_sine_of_its_deflection() {
        // The corner at (50, 0) turns 90 degrees left. The section at station
        // 40 (the vertical x = 40) and the key section at the corner, station
        // 50 (the horizontal y = 0, cut square to the leg after it) cross at
        // (40, 0): on the second 10 m from the centreline, 10 = interval /
        // sin 90 degrees. With the interval 10: a reach of 15 is refused,
        // naming 10, and 10 is allowed: the crossing is then at the section's
        // end.
        let alignment = through(&[
            (p2(0.0, 0.0), 0.0),
            (p2(50.0, 0.0), 0.0),
            (p2(50.0, 50.0), 0.0),
        ]);
        let base = slab(-100.0, -100.0, 300.0, 0.0);
        let top = slab(-100.0, -100.0, 300.0, 1.0);
        let mut options = AlignmentVolumeOptions {
            interval: 10.0,
            left: Some(15.0),
            right: Some(15.0),
            ..AlignmentVolumeOptions::default()
        };
        let refused = volumes(&base, &top, &alignment, &options).expect_err("crossing sections");
        assert!(
            refused.message.contains("at most 10.000 m"),
            "{}",
            refused.message
        );
        options.left = Some(10.0);
        options.right = Some(10.0);
        assert!(volumes(&base, &top, &alignment, &options).is_ok());
    }

    #[test]
    fn a_straight_alignment_with_the_default_reach_is_never_refused_for_crossing() {
        // Parallel sections never cross, whatever the reach or the interval: a
        // diagonal line, a section every half metre, each 141 m long.
        let base = slab(0.0, 0.0, 100.0, 10.0);
        let top = slab(0.0, 0.0, 100.0, 11.0);
        let options = AlignmentVolumeOptions {
            interval: 0.5,
            ..AlignmentVolumeOptions::default()
        };
        let result = must(volumes(
            &base,
            &top,
            &straight(p2(10.0, 10.0), p2(90.0, 70.0)),
            &options,
        ));
        assert!(result.sections.len() > 150);
    }

    // ---- nothing to measure ------------------------------------------------------------------------

    #[test]
    fn surfaces_whose_boxes_meet_but_whose_triangles_do_not_are_refused_naming_both_extents() {
        // a is the triangle (0,0) (60,0) (0,60), b the triangle (100,100)
        // (40,100) (100,40): their boxes meet on [40, 60]^2, and a lies below
        // x + y = 60 while b lies above x + y = 140, so they share no ground.
        // Every other VOLUME form refuses such a pair; this answered a cut and
        // a fill of 0.
        let a = build_from_points(vec![
            p3(0.0, 0.0, 0.0),
            p3(60.0, 0.0, 0.0),
            p3(0.0, 60.0, 0.0),
        ]);
        let b = build_from_points(vec![
            p3(100.0, 100.0, 1.0),
            p3(40.0, 100.0, 1.0),
            p3(100.0, 40.0, 1.0),
        ]);
        let refused = volumes(
            &a,
            &b,
            &straight(p2(0.0, 50.0), p2(100.0, 50.0)),
            &AlignmentVolumeOptions::default(),
        )
        .expect_err("no shared ground");
        assert_eq!(refused.code, ErrorCode::InvalidGeometry);
        assert!(
            refused.message.contains("0.000,0.000,60.000,60.000"),
            "{}",
            refused.message
        );
        assert!(
            refused.message.contains("40.000,40.000,100.000,100.000"),
            "{}",
            refused.message
        );
        // Boxes that do not meet name the extents too.
        let distant = plane_surface(Plane::new(0.0, 0.0, 0.0), 10.0, 5, 14, 500.0, 500.0);
        let apart = volumes(
            &a,
            &distant,
            &straight(p2(0.0, 50.0), p2(100.0, 50.0)),
            &AlignmentVolumeOptions::default(),
        )
        .expect_err("no shared ground");
        assert_eq!(apart.code, ErrorCode::InvalidGeometry);
        assert!(
            apart.message.contains("500.000,500.000,510.000,510.000"),
            "{}",
            apart.message
        );
    }

    #[test]
    fn an_alignment_that_runs_clear_of_the_common_ground_is_refused_but_one_over_part_of_it_is_not()
    {
        // Both surfaces are [0, 10]^2. Along y = 100 no section reaches them
        // (the default reach is the diagonal of the common ground, 14.1 m):
        // refused. Along y = 5 from x = -10 to 10 the sections at x = -10 and
        // -5 are off the ground and those at 0, 5 and 10 are on it: a part is
        // not a refusal, the empty ones are counted by the verb as
        // sections.empty.
        let base = slab(0.0, 0.0, 10.0, 0.0);
        let top = slab(0.0, 0.0, 10.0, 1.0);
        let options = AlignmentVolumeOptions {
            interval: 5.0,
            ..AlignmentVolumeOptions::default()
        };
        let clear = volumes(
            &base,
            &top,
            &straight(p2(0.0, 100.0), p2(10.0, 100.0)),
            &options,
        )
        .expect_err("clear of the ground");
        assert_eq!(clear.code, ErrorCode::InvalidGeometry);
        assert!(clear.message.contains("no section"), "{}", clear.message);

        let over = must(volumes(
            &base,
            &top,
            &straight(p2(0.0, 5.0), p2(10.0, 5.0)),
            &options,
        ));
        near(over.end_area_fill, 100.0, 1e-9);

        let partly = must(volumes(
            &base,
            &top,
            &straight(p2(-10.0, 5.0), p2(10.0, 5.0)),
            &options,
        ));
        assert_eq!(partly.sections.len(), 5);
        assert_eq!(partly.sections[0].width, 0.0);
        assert_eq!(partly.sections[1].width, 0.0);
        near(partly.sections[2].width, 10.0, 1e-9);
        near(partly.sections[4].width, 10.0, 1e-9);
    }

    // ---- the curvature correction ---------------------------------------------------------------

    #[test]
    fn a_strip_on_the_inside_of_a_quarter_circle_is_the_annulus_between_its_radii_not_the_centreline_arc()
     {
        // A 1 m slab over [-50, 150]^2, sections reaching 10 m to the left (the
        // inside: the centre is at (50, 50), on the left of the way) and none
        // to the right: a strip 10 wide and 1 deep, so 10 m2 on every section,
        // its centroid 5 m left of the centreline, its first moment 50 m3.
        // Exactly: the tangents hold 50 x 10 = 500 m3 each, and between the
        // radii 50 and 40 the quarter circle holds (pi/4)(50^2 - 40^2) =
        // 225 pi = 706.858: 1000 + 225 pi = 1706.858 in all. The centreline's
        // arc is 25 pi = 78.540 long, so the plain end area, 10 m2 over its
        // length, is 500 + 250 pi + 500 = 1785.398, 4.6 % over, since the
        // strip's centroid travels (R - 5) theta and not R theta (Pappus).
        // Corrected a panel on the arc is L (A - k M) = L (10 - 50/50) = 9 L,
        // and 9 x 25 pi = 225 pi. The area is constant, so the end-area rule
        // itself has nothing to discretise and the interval cannot matter: the
        // sum is exact to the rounding of the areas, 1e-12 each.
        let base = slab(-50.0, -50.0, 200.0, 0.0);
        let top = slab(-50.0, -50.0, 200.0, 1.0);
        let alignment = quarter_circle_left();
        near(alignment.length(), 100.0 + 25.0 * PI, 1e-9);
        for interval in [10.0, 1.0] {
            let on = must(volumes(
                &base,
                &top,
                &alignment,
                &strip_reach(10.0, 0.0, interval, true),
            ));
            assert!(on.curvature);
            assert!(
                (on.end_area_fill - (1000.0 + 225.0 * PI)).abs() <= 1e-7,
                "{interval}"
            );
            assert!(
                (on.end_area_shift.fill - (-25.0 * PI)).abs() <= 1e-7,
                "{interval}"
            );
            assert_eq!(on.end_area_cut, 0.0);
            assert_eq!(on.end_area_shift.cut, 0.0);
            for section in &on.sections {
                assert!(
                    (section.fill_area - 10.0).abs() <= 1e-9,
                    "{}",
                    section.station
                );
                assert!(
                    (section.fill_moment - 50.0).abs() <= 1e-9,
                    "{}",
                    section.station
                );
            }
            // The curvature is the arc's 1/50 on the panels whose middle is
            // between its ends (TS at station 50, ST at 50 + 25 pi), and 0 on
            // the tangents.
            let mut on_the_arc = false;
            let mut on_a_tangent = false;
            for panel in &on.panels {
                let middle = 0.5 * (panel.from_station + panel.to_station);
                let arc = middle > 50.0 && middle < 50.0 + 25.0 * PI;
                let expected = if arc { 0.02 } else { 0.0 };
                assert!(
                    (panel.curvature - expected).abs() <= 1e-12,
                    "{middle}: {}",
                    panel.curvature
                );
                on_the_arc = on_the_arc || arc;
                on_a_tangent = on_a_tangent || !arc;
            }
            assert!(on_the_arc);
            assert!(on_a_tangent);

            // Off: the plain figure along the centreline, nothing shifted.
            let off = must(volumes(
                &base,
                &top,
                &alignment,
                &strip_reach(10.0, 0.0, interval, false),
            ));
            assert!(!off.curvature);
            assert!(
                (off.end_area_fill - (1000.0 + 250.0 * PI)).abs() <= 1e-7,
                "{interval}"
            );
            assert_eq!(off.end_area_shift.fill, 0.0);
            for panel in &off.panels {
                assert_eq!(panel.curvature, 0.0);
            }
        }
    }

    #[test]
    fn a_strip_on_the_outside_of_a_quarter_circle_is_longer_than_its_centreline_arc() {
        // The same slab and way, the strip on the right, the outside: radii 50
        // to 60. Exactly 1000 + (pi/4)(60^2 - 50^2) = 1000 + 275 pi =
        // 1863.938, the plain 1785.398 4.2 % under. The strip's centroid is 5 m
        // right, M = -50, so a panel on the arc is L (10 + 50/50) = 11 L,
        // 11 x 25 pi = 275 pi.
        let base = slab(-50.0, -50.0, 200.0, 0.0);
        let top = slab(-50.0, -50.0, 200.0, 1.0);
        let outside = must(volumes(
            &base,
            &top,
            &quarter_circle_left(),
            &strip_reach(0.0, 10.0, 5.0, true),
        ));
        near(outside.end_area_fill, 1000.0 + 275.0 * PI, 1e-7);
        near(outside.end_area_shift.fill, 25.0 * PI, 1e-7);
        for section in &outside.sections {
            assert!(
                (section.fill_moment - (-50.0)).abs() <= 1e-9,
                "{}",
                section.station
            );
        }
        // Prismoidal, with a section cut halfway between each pair, is the same
        // constant area through Simpson's rule and takes the same correction.
        let both = AlignmentVolumeOptions {
            prismoidal: true,
            ..strip_reach(0.0, 10.0, 5.0, true)
        };
        let prismoidal = must(volumes(&base, &top, &quarter_circle_left(), &both));
        near(prismoidal.prismoidal_fill, 1000.0 + 275.0 * PI, 1e-7);
        near(prismoidal.prismoidal_shift.fill, 25.0 * PI, 1e-7);
    }

    #[test]
    fn a_strip_symmetric_about_the_centreline_is_the_same_with_or_without_the_correction() {
        // 10 m each side: 20 m2, centroid on the centreline, M = 0. The curve
        // holds (pi/4)(60^2 - 40^2) = 500 pi, the tangents 50 x 20 = 1000 each:
        // 2000 + 500 pi = 3570.796, which the centreline arc (25 pi x 20 =
        // 500 pi) gives as well.
        let base = slab(-50.0, -50.0, 200.0, 0.0);
        let top = slab(-50.0, -50.0, 200.0, 1.0);
        let on = must(volumes(
            &base,
            &top,
            &quarter_circle_left(),
            &strip_reach(10.0, 10.0, 5.0, true),
        ));
        let off = must(volumes(
            &base,
            &top,
            &quarter_circle_left(),
            &strip_reach(10.0, 10.0, 5.0, false),
        ));
        near(on.end_area_fill, 2000.0 + 500.0 * PI, 1e-7);
        near(off.end_area_fill, 2000.0 + 500.0 * PI, 1e-7);
        near(on.end_area_shift.fill, 0.0, 1e-9);
        for section in &on.sections {
            assert!(section.fill_moment.abs() <= 1e-9, "{}", section.station);
        }
    }

    #[test]
    fn the_same_strip_on_a_way_that_turns_right_has_its_inside_on_the_right() {
        // From (100, 100) south to the PI (100, 0) and west to (0, 0): the
        // centre (50, 50) is on the right. The right strip is the inside,
        // 1706.858, with its centroid at offset -5 (negative: right) and the
        // curvature -1/50; the left strip is the outside, 1863.938.
        let base = slab(-50.0, -50.0, 200.0, 0.0);
        let top = slab(-50.0, -50.0, 200.0, 1.0);
        let inside = must(volumes(
            &base,
            &top,
            &quarter_circle_right(),
            &strip_reach(0.0, 10.0, 5.0, true),
        ));
        near(inside.end_area_fill, 1000.0 + 225.0 * PI, 1e-7);
        for section in &inside.sections {
            assert!(
                (section.fill_moment - (-50.0)).abs() <= 1e-9,
                "{}",
                section.station
            );
        }
        let turning = inside
            .panels
            .iter()
            .filter(|panel| (panel.curvature + 0.02).abs() < 1e-12)
            .count();
        assert!(turning > 5);
        let outside = must(volumes(
            &base,
            &top,
            &quarter_circle_right(),
            &strip_reach(10.0, 0.0, 5.0, true),
        ));
        near(outside.end_area_fill, 1000.0 + 275.0 * PI, 1e-7);
    }

    #[test]
    fn on_a_straight_the_correction_changes_no_bit_of_an_asymmetric_section() {
        // Curvature 0 everywhere: the panel is the plain expression, bit for
        // bit, on an asymmetric reach and with the middle sections of the
        // prismoidal formula.
        let w = wedge();
        let alignment = straight(p2(0.0, 0.0), p2(100.0, 0.0));
        let mut options = AlignmentVolumeOptions {
            interval: 10.0,
            left: Some(5.0),
            right: Some(20.0),
            prismoidal: true,
            ..AlignmentVolumeOptions::default()
        };
        let on = must(volumes(&w.ground, &w.top, &alignment, &options));
        options.curvature = false;
        let off = must(volumes(&w.ground, &w.top, &alignment, &options));
        assert!(same_bits(on.end_area_fill, off.end_area_fill));
        assert!(same_bits(on.end_area_cut, off.end_area_cut));
        assert!(same_bits(on.prismoidal_fill, off.prismoidal_fill));
        assert_eq!(on.end_area_shift.fill, 0.0);
        assert_eq!(on.prismoidal_shift.fill, 0.0);
        // The wedge's sections are s/5 wide about the centreline, but only 5
        // of it are reached to the left: the moments are real, the correction
        // not applied.
        assert_ne!(on.sections[on.sections.len() - 1].fill_moment, 0.0);
    }

    #[test]
    fn a_strip_along_spirals_and_a_circle_loses_its_moment_times_the_turn_whatever_the_spirals_do()
    {
        // Tangent east, a spiral of 30 m into a circle of radius 100 and one
        // out of it, tangent north: the alignment turns left by the deflection
        // at the PI, 90 degrees, however it is shared between the spirals and
        // the circle. A strip 10 wide on the inside (left), 1 m deep: A = 10,
        // M = 50. The volume between two normals is the integral of A - k M,
        // so over the alignment A L - M (the angle turned) = 10 L - 50 pi/2,
        // and the plain figure less the corrected is M x pi/2 = 78.540, which
        // does not need the length at all. A panel's curvature is taken at its
        // middle, and curvature is linear on a spiral and constant on a circle,
        // so k(middle) L is the angle the panel turns exactly: the sum is the
        // turn.
        let definition = HorizontalAlignment {
            pis: vec![
                pi_at(p2(0.0, 0.0)),
                AlignmentPi {
                    point: p2(200.0, 0.0),
                    radius: 100.0,
                    spiral_in: 30.0,
                    spiral_out: 30.0,
                },
                pi_at(p2(200.0, 200.0)),
            ],
            ..HorizontalAlignment::default()
        };
        let alignment = must(solve_alignment(&definition));
        let base = slab(-50.0, -50.0, 300.0, 0.0);
        let top = slab(-50.0, -50.0, 300.0, 1.0);
        let mut options = AlignmentVolumeOptions {
            prismoidal: true,
            ..strip_reach(10.0, 0.0, 10.0, true)
        };
        let on = must(volumes(&base, &top, &alignment, &options));
        options.curvature = false;
        let off = must(volumes(&base, &top, &alignment, &options));
        let turn = PI / 2.0;
        near(off.end_area_fill - on.end_area_fill, 50.0 * turn, 1e-6);
        near(on.end_area_shift.fill, -50.0 * turn, 1e-6);
        near(on.prismoidal_shift.fill, -50.0 * turn, 1e-6);
        near(
            on.end_area_fill,
            10.0 * alignment.length() - 50.0 * turn,
            1e-6,
        );
    }

    #[test]
    fn a_sloping_surface_on_a_curve_is_corrected_to_its_exact_volume_as_the_interval_shrinks() {
        // The ground z = 0, the design z = 0.02 x, over the left strip 10 wide
        // of the quarter circle. Exactly (integrating 0.02 x over the ground):
        //   tangent east, x in [0, 50], y in [0, 10]: 0.02 x 10 x 1250 = 250;
        //   tangent north, x in [90, 100], y in [50, 100]: 0.02 x 50 x 950 = 950;
        //   the arc, x = 50 + r cos(phi), r in [40, 50], phi in [-pi/2, 0]:
        //     per metre of station the corrected integrand is the integral over
        //     the offset o in [0, 10] of 0.02 (50 + (50 - o) cos phi)(1 - o/50) do
        //       = 0.02 (450 + 406.667 cos phi) = 9 + 8.1333 cos phi,
        //     and ds = 50 dphi: 50 (9 pi/2 + 8.1333) = 1113.525.
        // 2313.525 in all. The panel sum is the trapezoid rule on that
        // integrand (the area is a function of station, so the end-area rule
        // is an approximation): its error per panel of length h is h^3 F''/12,
        // with |F''| = 8.1333/50^2 |cos phi| <= 0.003253 and the arc 78.540
        // long, so at most 78.540 x 0.003253 / 12 h^2 = 0.02129 h^2 over the
        // whole arc; the tangents, where the area is linear or constant in
        // station, are exact. Uncorrected, the integrand is the plain area
        // 10 + 9 cos phi: 50 (5 pi + 9) = 1235.398 on the arc, 2435.398 in all,
        // 5.3 % over, with the same bound (9/2500 for F'') 0.02356 h^2 about
        // its own, and it never gets closer to the exact figure, which is what
        // the correction is for.
        let base = slab(-50.0, -50.0, 200.0, 0.0);
        let top = plane_surface(Plane::new(0.02, 0.0, 0.0), 200.0, 40, 35, -50.0, -50.0);
        let alignment = quarter_circle_left();
        let exact = 250.0 + 950.0 + 50.0 * (4.5 * PI + 8.0 + 2.0 / 15.0);
        near(exact, 2313.525014, 1e-5); // the arithmetic above, 406.667 x 0.02 = 8 + 2/15
        let mut previous = 1e9;
        for interval in [2.0, 1.0, 0.25] {
            let on = must(volumes(
                &base,
                &top,
                &alignment,
                &strip_reach(10.0, 0.0, interval, true),
            ));
            let error = (on.end_area_fill - exact).abs();
            assert!(
                error <= 0.0215 * interval * interval + 1e-8,
                "{interval}: {error}"
            );
            assert!(error < previous, "{interval}"); // shrinking, as the rule says
            previous = error;
            let off = must(volumes(
                &base,
                &top,
                &alignment,
                &strip_reach(10.0, 0.0, interval, false),
            ));
            assert!(
                (off.end_area_fill - (1200.0 + 50.0 * (5.0 * PI + 9.0))).abs()
                    <= 0.0236 * interval * interval + 1e-8,
                "{interval}"
            );
            assert!(off.end_area_fill - exact > 100.0, "{interval}");
        }
        // Simpson's rule is exact to the fourth derivative: 5 m panels are
        // within 5^4 x 78.540 x (8.1333 / 50^4) / 2880 = 2.2e-5.
        let options = AlignmentVolumeOptions {
            prismoidal: true,
            ..strip_reach(10.0, 0.0, 5.0, true)
        };
        let prismoidal = must(volumes(&base, &top, &alignment, &options));
        near(prismoidal.prismoidal_fill, exact, 1e-4);
    }

    // ---- stretches that run side by side ---------------------------------------------------------

    #[test]
    fn two_legs_of_au_turn_closer_than_twice_the_reach_are_refused_naming_the_stretches() {
        // The legs along y = 0 and y = 15, 15 apart; sections every 20 m with a
        // reach of 10 each side, so the strips y in [-10, 10] and [5, 25] share
        // y in [5, 10] over the 100 m they run alongside, 500 m2 swept twice.
        // No section crosses another (the legs' are all parallel, and the
        // corners' reach is under the interval), so this is the case the
        // crossing test cannot see; the volume was the sum, with the shared
        // 5 m counted for each leg.
        let base = slab(-50.0, -50.0, 200.0, 0.0);
        let top = slab(-50.0, -50.0, 200.0, 1.0);
        let options = AlignmentVolumeOptions {
            interval: 20.0,
            left: Some(10.0),
            right: Some(10.0),
            ..AlignmentVolumeOptions::default()
        };
        let refused = volumes(&base, &top, &u_turn(), &options).expect_err("side by side");
        assert_eq!(refused.code, ErrorCode::InvalidArgument);
        let message = &refused.message;
        assert!(message.contains("side by side"), "{message}");
        // The first leg's stretch begins at the alignment's start and the
        // second leg's runs to its end, 100 + 15 + 100 = 215.
        assert!(message.contains("from station 0.000"), "{message}");
        assert!(message.contains("to 215.000"), "{message}");
        assert!(message.contains("left= and right="), "{message}");
        assert!(!message.contains("cross each other"), "{message}");
        assert_eq!(refused.context, "left=,right=");
        // The same pair named whatever the thread count.
        let serial = TaskPool::new(0);
        let many = TaskPool::new(4);
        let on_one = volumes_along_alignment(&base, &top, &u_turn(), &options, Some(&serial))
            .expect_err("side by side");
        let on_many = volumes_along_alignment(&base, &top, &u_turn(), &options, Some(&many))
            .expect_err("side by side");
        assert_eq!(&on_one.message, message);
        assert_eq!(&on_many.message, message);
    }

    #[test]
    fn strips_that_only_touch_or_clear_one_another_are_measured() {
        // Half the distance between the legs is 7.5. At 7 each way the strips
        // are 1 m apart; at exactly 7.5 they touch along y = 7.5 and share no
        // area; a hair over (1e-6 each way, 2e-6 of overlap, above the 1e-7 of
        // GEOMETRIC) they overlap, and 1e-8 each way (2e-8) is under it and is
        // touching.
        let base = slab(-50.0, -50.0, 200.0, 0.0);
        let top = slab(-50.0, -50.0, 200.0, 1.0);
        let with = |reach_each_way: f64| {
            let options = AlignmentVolumeOptions {
                interval: 20.0,
                left: Some(reach_each_way),
                right: Some(reach_each_way),
                ..AlignmentVolumeOptions::default()
            };
            volumes(&base, &top, &u_turn(), &options)
        };
        assert!(with(7.0).is_ok());
        assert!(with(7.5).is_ok());
        assert!(with(7.5 + 1e-8).is_ok());
        let overlapping = with(7.5 + 1e-6).expect_err("overlapping strips");
        assert!(overlapping.message.contains("side by side"));
        assert!(with(8.0).is_err());
    }

    #[test]
    fn legs_whose_strips_are_asymmetric_are_refused_only_when_the_strips_meet() {
        // The first leg runs east along y = 0 and the last west along y = 15.
        // The left of the first is north, toward the other leg, and the left of
        // the last is south, toward the first: left=12 reaches y = 12 on the
        // first and y = 3 on the last, which overlap in y = [3, 12]. The right
        // of each is away from the other (south of the first, north of the
        // last): right=12 gives strips y = [-12, 0] and [15, 27], apart.
        let base = slab(-50.0, -50.0, 200.0, 0.0);
        let top = slab(-50.0, -50.0, 200.0, 1.0);
        let towards = AlignmentVolumeOptions {
            interval: 20.0,
            left: Some(12.0),
            right: Some(0.0),
            ..AlignmentVolumeOptions::default()
        };
        assert!(volumes(&base, &top, &u_turn(), &towards).is_err());
        let away = AlignmentVolumeOptions {
            interval: 20.0,
            left: Some(0.0),
            right: Some(12.0),
            ..AlignmentVolumeOptions::default()
        };
        assert!(volumes(&base, &top, &u_turn(), &away).is_ok());
    }
}
