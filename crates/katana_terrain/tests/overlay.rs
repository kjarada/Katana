//! Twin of tests/terrain/test_overlay.cpp: the overlay of two surfaces, the
//! region it may be cut to, the pieces it hands out, and the difference and
//! envelope surfaces made of it. Every expected value is worked by hand from
//! planes and squares in the comments, or is another method's answer for the
//! same quantity (compare_surfaces, volume_to_datum), never a previous run's.

#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "The C++ tests' negated comparisons reject a NaN as the oracle's do."
)]
#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

pub mod support;

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use katana_core::task_pool::TaskPool;
use katana_core::{ErrorCode, Result};
use katana_geometry::{Point2, Point3, Polyline2};
use katana_terrain::TinSurface;
use katana_terrain::overlay::{
    Envelope, OverlayPiece, OverlayRegion, OverlaySurfaceOptions, OverlayVisitor,
    difference_surface, envelope_surface, overlay_block_count, visit_overlay,
};
use katana_terrain::volume::{compare_surfaces, volume_to_datum};
use support::*;

fn must<T>(result: Result<T>) -> T {
    result.unwrap_or_else(|error| panic!("{}", error.describe()))
}

fn ring(vertices: Vec<Point2>) -> Polyline2 {
    Polyline2::new(vertices, true)
}

fn square(x0: f64, y0: f64, size: f64) -> Polyline2 {
    ring(vec![
        p2(x0, y0),
        p2(x0 + size, y0),
        p2(x0 + size, y0 + size),
        p2(x0, y0 + size),
    ])
}

/// A plane over [x0, x0 + size]^2: its four corners and `interior` points on
/// it at random, so the triangulation is irregular but the surface is the
/// plane.
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

/// Rolling ground over [0, 200]^2: two sine terms, so a second one lifted a
/// little crosses it in many places and every sign case of the overlay runs.
fn rolling(seed: u64, interior: usize, lift: f64) -> TinSurface {
    let mut random = Random::new(seed);
    let height =
        |x: f64, y: f64| 30.0 + lift + 1.5 * (x * 0.07).sin() * (y * 0.05).cos() + 0.01 * x;
    let mut points: Vec<Point3> = Vec::new();
    for corner in [
        p2(0.0, 0.0),
        p2(200.0, 0.0),
        p2(200.0, 200.0),
        p2(0.0, 200.0),
    ] {
        points.push(p3(corner.x, corner.y, height(corner.x, corner.y)));
    }
    for _ in 0..interior {
        let x = random.real(1.0, 199.0);
        let y = random.real(1.0, 199.0);
        points.push(p3(x, y, height(x, y)));
    }
    build_from_points(points)
}

/// Adds up what it is told, block by block, and checks the blocks.
struct Tally {
    blocks: Vec<Mutex<(f64, usize)>>,
    differing: AtomicBool,
    empty_piece: AtomicBool,
}

impl Tally {
    fn new(blocks: usize) -> Self {
        Self {
            blocks: (0..blocks).map(|_| Mutex::new((0.0, 0))).collect(),
            differing: AtomicBool::new(false),
            empty_piece: AtomicBool::new(false),
        }
    }

    fn slot(&self, block: usize) -> (f64, usize) {
        *self.blocks[block]
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }

    fn area(&self) -> f64 {
        let mut total = 0.0;
        for block in 0..self.blocks.len() {
            total += self.slot(block).0;
        }
        total
    }

    fn pieces(&self) -> usize {
        let mut total = 0;
        for block in 0..self.blocks.len() {
            total += self.slot(block).1;
        }
        total
    }

    fn differing(&self) -> bool {
        self.differing.load(Ordering::Relaxed)
    }

    fn empty_piece(&self) -> bool {
        self.empty_piece.load(Ordering::Relaxed)
    }
}

impl OverlayVisitor for Tally {
    fn piece(&self, block: usize, piece: &OverlayPiece) {
        assert!(
            block < self.blocks.len(),
            "block {block} of {}",
            self.blocks.len()
        );
        {
            let mut slot = self.blocks[block]
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            slot.0 += piece.area;
            slot.1 += 1;
        }
        if piece.z_top != piece.z_base {
            self.differing.store(true, Ordering::Relaxed);
        }
        if !(piece.area > 0.0) {
            self.empty_piece.store(true, Ordering::Relaxed);
        }
    }
}

// ---- the region ---------------------------------------------------------------------------

mod overlay_region {
    use super::*;

    #[test]
    fn a_square_is_two_triangles_of_its_area() {
        let region = must(OverlayRegion::from_polygon(&square(0.0, 0.0, 10.0), &[]));
        assert_eq!(region.triangles().len(), 2);
        near(region.area(), 100.0, 1e-12);
        assert!(region.contains(p2(5.0, 5.0)));
        assert!(region.contains(p2(10.0, 5.0))); // the boundary counts
        assert!(!region.contains(p2(11.0, 5.0)));
        for triangle in region.triangles() {
            assert!(triangle.signed_area() > 0.0); // counter-clockwise
        }
    }

    #[test]
    fn a_hole_is_taken_out_of_the_area() {
        // 10 x 10 less a 2 x 2 hole: 96.
        let region = must(OverlayRegion::from_polygon(
            &square(0.0, 0.0, 10.0),
            &[square(4.0, 4.0, 2.0)],
        ));
        near(region.area(), 96.0, 1e-12);
        assert!(!region.contains(p2(5.0, 5.0)));
        assert!(region.contains(p2(4.0, 5.0))); // on the hole's edge: the region's boundary
        assert!(region.contains(p2(2.0, 2.0)));
    }

    #[test]
    fn a_ring_drawn_back_to_its_start_is_the_same_ring() {
        let mut closed_by_hand = square(0.0, 0.0, 10.0);
        closed_by_hand.vertices.push(closed_by_hand.vertices[0]);
        let region = must(OverlayRegion::from_polygon(&closed_by_hand, &[]));
        near(region.area(), 100.0, 1e-12);
    }

    #[test]
    fn a_non_convex_polygon_is_tiled_exactly() {
        // An L: 40 x 20 and 20 x 20 more, 1200.
        let region = must(OverlayRegion::from_polygon(
            &ring(vec![
                p2(10.0, 10.0),
                p2(50.0, 10.0),
                p2(50.0, 30.0),
                p2(30.0, 30.0),
                p2(30.0, 50.0),
                p2(10.0, 50.0),
            ]),
            &[],
        ));
        near(region.area(), 1200.0, 1e-10);
        assert!(!region.contains(p2(40.0, 40.0))); // in the notch
    }

    #[test]
    fn a_ring_that_crosses_itself_or_has_no_area_is_refused() {
        let bow_tie = OverlayRegion::from_polygon(
            &ring(vec![
                p2(0.0, 0.0),
                p2(10.0, 10.0),
                p2(10.0, 0.0),
                p2(0.0, 10.0),
            ]),
            &[],
        )
        .expect_err("a bow tie is refused");
        assert_eq!(bow_tie.code, ErrorCode::InvalidGeometry);
        let two = OverlayRegion::from_polygon(&ring(vec![p2(0.0, 0.0), p2(10.0, 0.0)]), &[])
            .expect_err("two vertices are refused");
        assert_eq!(two.code, ErrorCode::InvalidGeometry);
        let line = OverlayRegion::from_polygon(
            &ring(vec![p2(0.0, 0.0), p2(5.0, 0.0), p2(10.0, 0.0)]),
            &[],
        )
        .expect_err("a line is refused");
        assert_eq!(line.code, ErrorCode::InvalidGeometry);
    }

    #[test]
    fn at_a_level_it_is_a_flat_surface_of_its_area() {
        let region = must(OverlayRegion::from_polygon(&square(0.0, 0.0, 10.0), &[]));
        let flat = must(region.at_level(12.5));
        near(flat.plan_area(), 100.0, 1e-12);
        for v in flat.vertices() {
            assert_eq!(v.z, 12.5);
        }
        assert!(region.at_level(f64::NAN).is_err());
    }
}

// ---- the pieces -------------------------------------------------------------------------------

mod overlay {
    use super::*;

    #[test]
    fn a_surface_with_itself_is_tiled_by_pieces_of_no_difference() {
        // Every piece is a triangle of both, read at its own corners: the same
        // height from both sides, bit for bit, and the pieces cover the
        // surface's plan area, 120^2.
        let surface = plane_surface(Plane::new(0.2, -0.15, 30.0), 120.0, 300, 7, 0.0, 0.0);
        let tally = Tally::new(overlay_block_count(&surface));
        must(visit_overlay(&surface, &surface, &tally, None, None));
        near(tally.area(), 14400.0, 1e-8);
        assert!(!tally.differing());
        assert!(!tally.empty_piece());
    }

    #[test]
    fn a_region_cuts_the_pieces_to_its_own_area() {
        // Two irregular planes over [0, 100]^2 and a triangle inside both whose
        // edges cross their triangles anywhere: its shoelace area,
        // 1/2 |(b - a) x (c - a)| = 1/2 |67.6 x 67.6 - 14.7 x 26.7| = 2088.635.
        let base = plane_surface(Plane::new(0.01, 0.02, 5.0), 100.0, 200, 3, 0.0, 0.0);
        let top = plane_surface(Plane::new(-0.01, 0.0, 6.0), 100.0, 150, 4, 0.0, 0.0);
        let region = must(OverlayRegion::from_polygon(
            &ring(vec![p2(13.7, 21.2), p2(81.3, 35.9), p2(40.4, 88.8)]),
            &[],
        ));
        let expected =
            0.5 * ((81.3_f64 - 13.7) * (88.8 - 21.2) - (35.9 - 21.2) * (40.4 - 13.7)).abs();
        let tally = Tally::new(overlay_block_count(&base));
        must(visit_overlay(&base, &top, &tally, Some(&region), None));
        near(tally.area(), expected, 1e-8);
        near(region.area(), expected, 1e-9);
    }

    #[test]
    fn surfaces_that_do_not_meet_visit_nothing() {
        let west = plane_surface(Plane::new(0.0, 0.0, 1.0), 10.0, 5, 1, 0.0, 0.0);
        let east = plane_surface(Plane::new(0.0, 0.0, 2.0), 10.0, 5, 2, 50.0, 0.0);
        let tally = Tally::new(overlay_block_count(&west));
        must(visit_overlay(&west, &east, &tally, None, None));
        assert_eq!(tally.pieces(), 0);
    }

    #[test]
    fn an_empty_surface_is_refused() {
        let surface = plane_surface(Plane::new(0.0, 0.0, 1.0), 10.0, 5, 1, 0.0, 0.0);
        let tally = Tally::new(1);
        let error = visit_overlay(&surface, &TinSurface::new(), &tally, None, None)
            .expect_err("an empty surface is refused");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
    }
}

// ---- the difference surface --------------------------------------------------------------------

mod difference_surface {
    use super::*;

    #[test]
    fn two_parallel_planes_differ_by_their_separation_everywhere() {
        // The top is the base plane raised 1 m, triangulated from other
        // points: the difference is 1 at every vertex, over the 100 x 100 they
        // share.
        let ground = Plane::new(0.05, 0.02, 10.0);
        let base = plane_surface(ground, 100.0, 120, 21, 0.0, 0.0);
        let top = plane_surface(Plane::new(0.05, 0.02, 11.0), 100.0, 90, 22, 0.0, 0.0);
        let difference = must(difference_surface(
            &base,
            &top,
            &OverlaySurfaceOptions::default(),
        ));
        assert!(!difference.retriangulated);
        for v in difference.surface.vertices() {
            // Heights near 10 to 17, so each is read to a few ulps of 16.
            assert!((v.z - 1.0).abs() <= 1e-12, "{},{}: {}", v.x, v.y, v.z);
        }
        near(difference.surface.plan_area(), 10000.0, 1e-8);
        let volume = must(volume_to_datum(&difference.surface, 0.0));
        near(volume.above, 10000.0, 1e-8);
        assert_eq!(volume.below, 0.0);
    }

    #[test]
    fn it_integrates_to_what_compare_surfaces_measures() {
        // The two methods cut the same pieces; the surface carries one height
        // per corner where the comparison reads each piece's own, which differ
        // in the last bits. Volumes of order 1e4 over ~2e4 pieces: 1e-6.
        let base = rolling(31, 1500, 0.0);
        let top = rolling(37, 1200, 0.3);
        let comparison = must(compare_surfaces(&base, &top, None));
        assert!(comparison.cut > 1.0); // both signs: the two cross
        assert!(comparison.fill > 1.0);
        let difference = must(difference_surface(
            &base,
            &top,
            &OverlaySurfaceOptions::default(),
        ));
        let volume = must(volume_to_datum(&difference.surface, 0.0));
        near(volume.above, comparison.fill, 1e-6);
        near(volume.below, comparison.cut, 1e-6);
        near(volume.net, comparison.net, 1e-6);
        near(difference.surface.plan_area(), comparison.plan_area, 1e-6);
    }

    #[test]
    fn is_the_same_surface_at_any_thread_count() {
        let base = rolling(41, 2500, 0.0);
        let top = rolling(43, 1800, 0.2);
        let serial = TaskPool::new(0);
        let one = OverlaySurfaceOptions {
            pool: Some(&serial),
            ..OverlaySurfaceOptions::default()
        };
        let first = must(difference_surface(&base, &top, &one));
        for workers in [1, 3, 7] {
            let pool = TaskPool::new(workers);
            let many = OverlaySurfaceOptions {
                pool: Some(&pool),
                ..OverlaySurfaceOptions::default()
            };
            let again = must(difference_surface(&base, &top, &many));
            assert!(again.surface == first.surface, "{workers} workers");
        }
    }

    #[test]
    fn made_through_the_triangulation_it_measures_the_same() {
        // The fallback path, asked for: the same vertices and every piece edge
        // constrained, so the same surface up to the triangulator's merges.
        let base = rolling(51, 400, 0.0);
        let top = rolling(53, 300, 0.25);
        let direct = must(difference_surface(
            &base,
            &top,
            &OverlaySurfaceOptions::default(),
        ));
        let options = OverlaySurfaceOptions {
            retriangulate: true,
            ..OverlaySurfaceOptions::default()
        };
        let rebuilt = must(difference_surface(&base, &top, &options));
        assert!(!direct.retriangulated);
        assert!(rebuilt.retriangulated);
        let a = must(volume_to_datum(&direct.surface, 0.0));
        let b = must(volume_to_datum(&rebuilt.surface, 0.0));
        near(a.above, b.above, 1e-6);
        near(a.below, b.below, 1e-6);
        near(
            direct.surface.plan_area(),
            rebuilt.surface.plan_area(),
            1e-6,
        );
    }

    #[test]
    fn surfaces_with_no_ground_in_common_or_none_at_all_are_refused() {
        let west = plane_surface(Plane::new(0.0, 0.0, 1.0), 10.0, 5, 1, 0.0, 0.0);
        let east = plane_surface(Plane::new(0.0, 0.0, 2.0), 10.0, 5, 2, 50.0, 0.0);
        let apart = difference_surface(&west, &east, &OverlaySurfaceOptions::default())
            .expect_err("no ground in common");
        assert_eq!(apart.code, ErrorCode::InvalidGeometry);
        let empty =
            difference_surface(&west, &TinSurface::new(), &OverlaySurfaceOptions::default())
                .expect_err("an empty surface");
        assert_eq!(empty.code, ErrorCode::InvalidArgument);
    }
}

// ---- the envelope -----------------------------------------------------------------------------

mod envelope_surface {
    use super::*;

    /// z = 0 and z = x - 50 over [0, 100]^2, each from its four corners: they
    /// cross along x = 50.
    fn crossing_planes() -> (TinSurface, TinSurface) {
        let mut random = Random::default();
        let level = build_from_points(plane_points(
            &Plane::new(0.0, 0.0, 0.0),
            100.0,
            0,
            &mut random,
        ));
        let tilted = build_from_points(plane_points(
            &Plane::new(1.0, 0.0, -50.0),
            100.0,
            0,
            &mut random,
        ));
        (level, tilted)
    }

    #[test]
    fn the_upper_envelope_of_two_crossing_planes_is_the_higher_of_them() {
        let (level, tilted) = crossing_planes();
        let upper = must(envelope_surface(
            &level,
            &tilted,
            Envelope::Upper,
            &OverlaySurfaceOptions::default(),
        ));
        // max(0, x - 50): 0 west of x = 50, x - 50 east of it. Its volume is
        // 100 (the y extent) x 50^2 / 2 = 125 000, all above 0.
        let volume = must(volume_to_datum(&upper.surface, 0.0));
        near(volume.above, 125000.0, 1e-7);
        // Where they meet the height is read along an edge of the overlay, to
        // an ulp or so of 50 either side of 0.
        near(volume.below, 0.0, 1e-9);
        near(
            upper.surface.elevation_at(p2(75.0, 20.0)).expect("on it"),
            25.0,
            1e-12,
        );
        near(
            upper.surface.elevation_at(p2(25.0, 20.0)).expect("on it"),
            0.0,
            1e-12,
        );
        near(upper.surface.plan_area(), 10000.0, 1e-8);
    }

    #[test]
    fn the_lower_envelope_of_two_crossing_planes_is_the_lower_of_them() {
        let (level, tilted) = crossing_planes();
        let lower = must(envelope_surface(
            &level,
            &tilted,
            Envelope::Lower,
            &OverlaySurfaceOptions::default(),
        ));
        // min(0, x - 50): the mirror of the upper one, 125 000 below 0.
        let volume = must(volume_to_datum(&lower.surface, 0.0));
        near(volume.below, 125000.0, 1e-7);
        near(volume.above, 0.0, 1e-9);
        near(
            lower.surface.elevation_at(p2(25.0, 80.0)).expect("on it"),
            -25.0,
            1e-12,
        );
    }

    #[test]
    fn upper_over_lower_is_the_magnitude_of_the_difference_and_neither_dips_below_the_other() {
        // On rolling ground that crosses its lifted copy many times: upper
        // minus lower is |top - base|, so its fill is the comparison's cut
        // plus fill, and the upper envelope is nowhere below either surface.
        let base = rolling(61, 800, 0.0);
        let top = rolling(67, 700, 0.3);
        let comparison = must(compare_surfaces(&base, &top, None));
        let upper = must(envelope_surface(
            &base,
            &top,
            Envelope::Upper,
            &OverlaySurfaceOptions::default(),
        ));
        let lower = must(envelope_surface(
            &base,
            &top,
            Envelope::Lower,
            &OverlaySurfaceOptions::default(),
        ));
        let spread = must(compare_surfaces(&lower.surface, &upper.surface, None));
        near(spread.fill, comparison.fill + comparison.cut, 1e-6);
        near(spread.cut, 0.0, 1e-6);
        for surface in [&base, &top] {
            let above = must(compare_surfaces(&upper.surface, surface, None));
            near(above.fill, 0.0, 1e-6); // nothing of either rises above it
        }
    }

    #[test]
    fn surfaces_with_no_ground_in_common_are_refused() {
        let west = plane_surface(Plane::new(0.0, 0.0, 1.0), 10.0, 5, 1, 0.0, 0.0);
        let east = plane_surface(Plane::new(0.0, 0.0, 2.0), 10.0, 5, 2, 50.0, 0.0);
        let apart = envelope_surface(
            &west,
            &east,
            Envelope::Upper,
            &OverlaySurfaceOptions::default(),
        )
        .expect_err("no ground in common");
        assert_eq!(apart.code, ErrorCode::InvalidGeometry);
    }
}
