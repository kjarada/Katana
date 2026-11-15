use crate::support::{double_eq, near, vec2_eq};
use katana_core::ErrorCode;
use katana_geometry::editing::*;
use katana_geometry::{Arc2, Circle2, Line2, Point2, Polyline2, Segment2};
use katana_math::numerics::{HALF_PI, PI, tolerance};
fn point(east: f64, north: f64) -> Point2 {
    Point2::new(east, north)
}
fn segment(first: (f64, f64), last: (f64, f64)) -> Segment2 {
    Segment2::new(point(first.0, first.1), point(last.0, last.1))
}
fn curve(first: (f64, f64), last: (f64, f64)) -> Curve2 {
    Curve2::Segment(segment(first, last))
}
fn get_segment(curve: Curve2) -> Segment2 {
    let Curve2::Segment(segment) = curve else {
        panic!("expected segment")
    };
    segment
}
fn get_arc(curve: Curve2) -> Arc2 {
    let Curve2::Arc(arc) = curve else {
        panic!("expected arc")
    };
    arc
}
fn get_circle(curve: Curve2) -> Circle2 {
    let Curve2::Circle(circle) = curve else {
        panic!("expected circle")
    };
    circle
}
mod editing_offset {
    use super::*;
    #[test]
    fn segment_offsets_to_the_left_for_positive_distance() {
        let east = segment((0.0, 0.0), (10.0, 0.0));
        assert_eq!(offset(&east, 2.0), segment((0.0, 2.0), (10.0, 2.0)));
        assert_eq!(offset(&east, -2.0), segment((0.0, -2.0), (10.0, -2.0)));
        assert_eq!(
            offset(&east.reversed(), 2.0),
            segment((10.0, -2.0), (0.0, -2.0))
        );
    }
    #[test]
    fn circle_and_arc_change_radius_and_reject_collapse() {
        let circle = Circle2::new(point(1.0, 1.0), 5.0);
        assert!(offset(&circle, 2.0).is_some());
        double_eq(offset(&circle, 2.0).unwrap().radius, 7.0);
        double_eq(offset(&circle, -2.0).unwrap().radius, 3.0);
        assert!(offset(&circle, -5.0).is_none());
        assert!(offset(&circle, -6.0).is_none());
        let arc = Arc2::new(point(0.0, 0.0), 5.0, 0.0, HALF_PI);
        let grown = offset(&arc, 1.0).unwrap();
        double_eq(grown.radius, 6.0);
        double_eq(grown.sweep, arc.sweep);
        assert!(offset(&arc, -5.0).is_none());
    }
    #[test]
    fn polyline_uses_mitre_joins() {
        let corner = Polyline2::new(
            vec![point(0.0, 0.0), point(10.0, 0.0), point(10.0, 10.0)],
            false,
        );
        let left = offset(&corner, 1.0).unwrap();
        assert_eq!(left.vertices.len(), 3);
        assert!(vec2_eq(left.vertices[0], point(0.0, 1.0)));
        assert!(vec2_eq(left.vertices[1], point(9.0, 1.0)));
        assert!(vec2_eq(left.vertices[2], point(9.0, 10.0)));
        let right = offset(&corner, -1.0).unwrap();
        assert!(vec2_eq(right.vertices[1], point(11.0, -1.0)));
    }
    #[test]
    fn closed_polyline_shrinks_and_grows() {
        let square = Polyline2::new(
            vec![
                point(0.0, 0.0),
                point(10.0, 0.0),
                point(10.0, 10.0),
                point(0.0, 10.0),
            ],
            true,
        );
        let inner = offset(&square, 1.0).unwrap();
        near(inner.area(), 64.0, 1e-12);
        let outer = offset(&square, -1.0).unwrap();
        near(outer.area(), 144.0, 1e-12);
    }
    #[test]
    fn polyline_edge_cases() {
        let straight = Polyline2::new(
            vec![point(0.0, 0.0), point(5.0, 0.0), point(10.0, 0.0)],
            false,
        );
        let moved = offset(&straight, 1.0).unwrap();
        assert!(vec2_eq(moved.vertices[1], point(5.0, 1.0)));
        let duplicated = Polyline2::new(
            vec![point(0.0, 0.0), point(0.0, 0.0), point(10.0, 0.0)],
            false,
        );
        let deduplicated = offset(&duplicated, 1.0).unwrap();
        assert_eq!(deduplicated.vertices.len(), 2);
        let too_short = offset(&Polyline2::new(vec![point(1.0, 1.0)], false), 1.0).unwrap_err();
        assert_eq!(too_short.code, ErrorCode::InvalidGeometry);
        let reversal = Polyline2::new(
            vec![point(0.0, 0.0), point(10.0, 0.0), point(5.0, 0.0)],
            false,
        );
        assert!(offset(&reversal, 1.0).is_err());
    }
    #[test]
    fn offset_towards_picks_the_side() {
        let segment = curve((0.0, 0.0), (10.0, 0.0));
        let below = offset_towards(&segment, 2.0, point(5.0, -7.0)).unwrap();
        assert_eq!(
            get_segment(below),
            super::segment((0.0, -2.0), (10.0, -2.0))
        );
        let circle = Curve2::Circle(Circle2::new(point(0.0, 0.0), 5.0));
        double_eq(
            get_circle(offset_towards(&circle, 2.0, point(1.0, 0.0)).unwrap()).radius,
            3.0,
        );
        double_eq(
            get_circle(offset_towards(&circle, -2.0, point(9.0, 0.0)).unwrap()).radius,
            7.0,
        );
        assert!(offset_towards(&circle, 6.0, point(1.0, 0.0)).is_err());
        assert!(offset_towards(&segment, 0.0, point(1.0, 1.0)).is_err());
    }
}
mod editing_trim {
    use super::*;
    #[test]
    fn segment_between_two_cutters_loses_its_middle() {
        let target = curve((0.0, 0.0), (10.0, 0.0));
        let cutters = [
            curve((3.0, -1.0), (3.0, 1.0)),
            curve((7.0, -1.0), (7.0, 1.0)),
        ];
        let result = trim(&target, &cutters, point(5.0, 0.2)).unwrap();
        assert_eq!(result.remaining.len(), 2);
        assert_eq!(
            get_segment(result.remaining[0]),
            segment((0.0, 0.0), (3.0, 0.0))
        );
        assert_eq!(
            get_segment(result.remaining[1]),
            segment((7.0, 0.0), (10.0, 0.0))
        );
    }
    #[test]
    fn segment_end_is_removed_up_to_the_nearest_cutter() {
        let target = curve((0.0, 0.0), (10.0, 0.0));
        let cutters = [
            curve((3.0, -1.0), (3.0, 1.0)),
            Curve2::Circle(Circle2::new(point(7.0, 0.0), 1.0)),
        ];
        let result = trim(&target, &cutters, point(9.5, 0.0)).unwrap();
        assert_eq!(result.remaining.len(), 1);
        let kept = get_segment(result.remaining[0]);
        assert!(vec2_eq(kept.start, point(0.0, 0.0)));
        assert!(vec2_eq(kept.end, point(8.0, 0.0)));
    }
    #[test]
    fn fails_when_nothing_crosses_the_target() {
        let target = curve((0.0, 0.0), (10.0, 0.0));
        let missing = [curve((3.0, 5.0), (3.0, 6.0))];
        let error = trim(&target, &missing, point(5.0, 0.0)).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidGeometry);
        let at_end = [curve((10.0, -1.0), (10.0, 1.0))];
        assert!(trim(&target, &at_end, point(5.0, 0.0)).is_err());
        assert!(trim(&target, &[], point(5.0, 0.0)).is_err());
    }
    #[test]
    fn arc_is_cut_at_its_crossing() {
        let target = Curve2::Arc(Arc2::new(point(0.0, 0.0), 5.0, 0.0, PI));
        let cutters = [curve((0.0, 0.0), (0.0, 10.0))];
        let result = trim(&target, &cutters, point(-4.0, 3.0)).unwrap();
        assert_eq!(result.remaining.len(), 1);
        let kept = get_arc(result.remaining[0]);
        near(kept.start_angle, 0.0, 1e-12);
        near(kept.sweep, HALF_PI, 1e-12);
    }
    #[test]
    fn circle_becomes_an_arc() {
        let target = Curve2::Circle(Circle2::new(point(0.0, 0.0), 5.0));
        let cutters = [curve((-10.0, 0.0), (10.0, 0.0))];
        let upper_removed = trim(&target, &cutters, point(0.0, 5.0)).unwrap();
        let lower = get_arc(upper_removed.remaining[0]);
        near(lower.start_angle, PI, 1e-12);
        near(lower.sweep, PI, 1e-12);
        assert!(vec2_eq(lower.midpoint(), point(0.0, -5.0)));
        let lower_removed = trim(&target, &cutters, point(0.0, -5.0)).unwrap();
        assert!(vec2_eq(
            get_arc(lower_removed.remaining[0]).midpoint(),
            point(0.0, 5.0)
        ));
        let tangent = [curve((-10.0, 5.0), (10.0, 5.0))];
        assert!(trim(&target, &tangent, point(0.0, 5.0)).is_err());
    }
}
mod editing_extend {
    use super::*;
    #[test]
    fn segment_reaches_the_first_boundary_ahead() {
        let target = curve((0.0, 0.0), (4.0, 0.0));
        let boundaries = [
            curve((10.0, -5.0), (10.0, 5.0)),
            curve((7.0, -5.0), (7.0, 5.0)),
            curve((-3.0, -5.0), (-3.0, 5.0)),
        ];
        let forward = extend(&target, &boundaries, point(3.9, 0.0)).unwrap();
        assert_eq!(get_segment(forward), segment((0.0, 0.0), (7.0, 0.0)));
        let backward = extend(&target, &boundaries, point(0.1, 0.0)).unwrap();
        assert_eq!(get_segment(backward), segment((-3.0, 0.0), (4.0, 0.0)));
    }
    #[test]
    fn fails_without_a_boundary_ahead() {
        let target = curve((0.0, 0.0), (4.0, 0.0));
        let behind = [curve((-3.0, -5.0), (-3.0, 5.0))];
        assert!(extend(&target, &behind, point(4.0, 0.0)).is_err());
        let touching = [curve((4.0, -5.0), (4.0, 5.0))];
        assert!(extend(&target, &touching, point(4.0, 0.0)).is_err());
        assert!(
            extend(
                &Curve2::Circle(Circle2::new(point(0.0, 0.0), 1.0)),
                &touching,
                point(1.0, 0.0)
            )
            .is_err()
        );
    }
    #[test]
    fn arc_grows_along_its_circle() {
        let target = Curve2::Arc(Arc2::new(point(0.0, 0.0), 5.0, 0.0, HALF_PI));
        let boundaries = [curve((-10.0, 0.0), (10.0, 0.0))];
        let grown_end = get_arc(extend(&target, &boundaries, point(0.0, 5.0)).unwrap());
        near(grown_end.sweep, PI, 1e-12);
        near(grown_end.start_angle, 0.0, 1e-12);
        let grown_start = get_arc(extend(&target, &boundaries, point(5.0, 0.0)).unwrap());
        assert!(vec2_eq(grown_start.start_point(), point(-5.0, 0.0)));
        assert!(vec2_eq(grown_start.end_point(), point(0.0, 5.0)));
        near(grown_start.sweep, 1.5 * PI, 1e-12);
    }
}
mod editing_fillet {
    use super::*;
    #[test]
    fn right_angle_corner_is_tangent_to_both_segments() {
        let result = fillet(
            &segment((10.0, 0.0), (0.0, 0.0)),
            &segment((0.0, 0.0), (0.0, 10.0)),
            2.0,
        )
        .unwrap();
        assert_eq!(result.first.start, point(10.0, 0.0));
        assert!(vec2_eq(result.first.end, point(2.0, 0.0)));
        assert!(vec2_eq(result.second.start, point(0.0, 2.0)));
        assert_eq!(result.second.end, point(0.0, 10.0));
        let arc = result.arc.unwrap();
        assert!(vec2_eq(arc.center, point(2.0, 2.0)));
        double_eq(arc.radius, 2.0);
        assert!(vec2_eq(arc.start_point(), point(2.0, 0.0)));
        assert!(vec2_eq(arc.end_point(), point(0.0, 2.0)));
        near(arc.sweep.abs(), HALF_PI, 1e-12);
        near(
            arc.midpoint().distance_to(point(0.0, 0.0)),
            2.0 * 2.0_f64.sqrt() - 2.0,
            1e-12,
        );
    }
    #[test]
    fn extends_segments_that_stop_short_of_the_corner() {
        let sharp = fillet(
            &segment((10.0, 0.0), (5.0, 0.0)),
            &segment((0.0, 5.0), (0.0, 10.0)),
            0.0,
        )
        .unwrap();
        assert!(sharp.arc.is_none());
        assert_eq!(sharp.first, segment((10.0, 0.0), (0.0, 0.0)));
        assert_eq!(sharp.second, segment((0.0, 0.0), (0.0, 10.0)));
    }
    #[test]
    fn rejects_parallel_oversized_and_negative() {
        let first = segment((0.0, 0.0), (10.0, 0.0));
        assert!(fillet(&first, &segment((0.0, 5.0), (10.0, 5.0)), 1.0).is_err());
        assert!(fillet(&first, &segment((0.0, 0.0), (0.0, 10.0)), 50.0).is_err());
        assert!(fillet(&first, &segment((0.0, 0.0), (0.0, 10.0)), -1.0).is_err());
        assert!(fillet(&first, &segment((3.0, 3.0), (3.0, 3.0)), 1.0).is_err());
        assert_eq!(
            fillet(&first, &segment((0.0, 0.0), (0.0, 10.0)), 50.0)
                .unwrap_err()
                .code,
            ErrorCode::InvalidGeometry
        );
    }
    #[test]
    fn acute_corner_geometry() {
        let first = segment((0.0, 0.0), (10.0, 0.0));
        let second = segment((0.0, 0.0), (5.0, 5.0 * 3.0_f64.sqrt()));
        let result = fillet(&first, &second, 1.0).unwrap();
        near(result.first.start.x, 3.0_f64.sqrt(), 1e-12);
        let arc = result.arc.unwrap();
        near(arc.center.distance_to(point(0.0, 0.0)), 2.0, 1e-12);
        near(arc.sweep.abs(), PI - PI / 3.0, 1e-12);
        near(
            Line2 {
                origin: first.start,
                direction: first.delta(),
            }
            .distance_to(arc.center),
            1.0,
            1e-12,
        );
        near(
            Line2 {
                origin: second.start,
                direction: second.delta(),
            }
            .distance_to(arc.center),
            1.0,
            1e-12,
        );
    }
}
mod editing_chamfer {
    use super::*;
    #[test]
    fn cuts_both_segments_and_adds_a_bevel() {
        let first = segment((10.0, 0.0), (0.0, 0.0));
        let second = segment((0.0, 0.0), (0.0, 10.0));
        let result = chamfer(&first, &second, 2.0, 3.0).unwrap();
        assert_eq!(result.first, segment((10.0, 0.0), (2.0, 0.0)));
        assert_eq!(result.second, segment((0.0, 3.0), (0.0, 10.0)));
        assert_eq!(result.bevel.unwrap(), segment((2.0, 0.0), (0.0, 3.0)));
        let sharp = chamfer(&first, &second, 0.0, 0.0).unwrap();
        assert!(sharp.bevel.is_none());
        assert!(chamfer(&first, &second, 11.0, 1.0).is_err());
        assert!(chamfer(&first, &second, -1.0, 1.0).is_err());
    }
}
mod editing_degenerate {
    use super::*;
    #[test]
    fn a_cut_that_consumes_the_whole_segment_is_refused() {
        let first = segment((0.0, 0.0), (10.0, 0.0));
        let second = segment((0.0, 0.0), (0.0, 10.0));
        let exact = chamfer(&first, &second, 10.0, 10.0).unwrap_err();
        assert_eq!(exact.code, ErrorCode::InvalidGeometry);
        assert!(exact.message.contains("segment"));
        assert!(chamfer(&first, &second, 10.0 - 0.5 * tolerance::GEOMETRIC, 1.0).is_err());
        let result = chamfer(&first, &second, 3.0, 3.0).unwrap();
        assert!(result.first.length() > 0.0);
        assert!(result.second.length() > 0.0);
    }
    #[test]
    fn a_fillet_that_consumes_the_whole_segment_is_refused() {
        let first = segment((0.0, 0.0), (10.0, 0.0));
        let second = segment((0.0, 0.0), (0.0, 10.0));
        let exact = fillet(&first, &second, 10.0).unwrap_err();
        assert_eq!(exact.code, ErrorCode::InvalidGeometry);
        let result = fillet(&first, &second, 2.0).unwrap();
        assert!(result.first.length() > 0.0);
        assert!(result.second.length() > 0.0);
        let arc = result.arc.unwrap();
        near(arc.center.x, 2.0, 1e-12);
        near(arc.center.y, 2.0, 1e-12);
    }
    #[test]
    fn collinear_segments_cannot_be_filleted() {
        assert!(
            fillet(
                &segment((0.0, 0.0), (10.0, 0.0)),
                &segment((10.0, 0.0), (20.0, 0.0)),
                1.0
            )
            .is_err()
        );
    }
}
