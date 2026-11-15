mod polyline_vertices {
    use crate::support::{double_eq, near};
    use katana_geometry::polyline_vertices::*;
    use katana_geometry::{CurvePolyline2, CurveSegment, Point2, Vec2};
    use katana_math::numerics::PI;
    fn point(east: f64, north: f64) -> Point2 {
        Point2::new(east, north)
    }
    fn line3d(points: &[(f64, f64)], heights: &[Option<f64>]) -> CurvePolyline2 {
        let positions: Vec<_> = points
            .iter()
            .map(|&(east, north)| point(east, north))
            .collect();
        let mut polyline = CurvePolyline2::from_points(&positions, false);
        for (vertex, &height) in polyline.vertices.iter_mut().zip(heights) {
            vertex.height = height;
        }
        polyline
    }
    fn expect_near(actual: Point2, expected: Point2) {
        near(actual.x, expected.x, 1e-9);
        near(actual.y, expected.y, 1e-9);
    }
    #[test]
    fn insert_on_a_straight_segment_interpolates_the_height() {
        let polyline = line3d(&[(0.0, 0.0), (10.0, 0.0)], &[Some(100.0), Some(110.0)]);
        let output = insert_vertex(&polyline, 0, point(4.0, 0.0), None).unwrap();
        assert_eq!(output.vertices.len(), 3);
        expect_near(output.vertices[1].position, point(4.0, 0.0));
        near(output.vertices[1].height.unwrap(), 104.0, 1e-12);
        let bent = insert_vertex(&polyline, 0, point(5.0, 3.0), Some(99.0)).unwrap();
        expect_near(bent.vertices[1].position, point(5.0, 3.0));
        assert_eq!(bent.vertices[1].height, Some(99.0));
        assert!(insert_vertex(&polyline, 0, point(0.0, 0.0), None).is_err());
        assert!(insert_vertex(&polyline, 1, point(5.0, 0.0), None).is_err());
    }
    #[test]
    fn insert_on_an_arc_keeps_the_circle() {
        let mut polyline = line3d(&[(0.0, 0.0), (2.0, 0.0)], &[]);
        polyline.vertices[0].bulge = 1.0;
        let output = insert_vertex(&polyline, 0, point(1.0, -5.0), None).unwrap();
        assert_eq!(output.vertices.len(), 3);
        expect_near(output.vertices[1].position, point(1.0, -1.0));
        near(output.vertices[0].bulge, (PI / 8.0).tan(), 1e-12);
        near(output.vertices[1].bulge, (PI / 8.0).tan(), 1e-12);
        near(output.length(), polyline.length(), 1e-9);
    }
    #[test]
    fn delete_joins_the_neighbours_straight_and_keeps_a_polyline() {
        let mut polyline = line3d(&[(0.0, 0.0), (5.0, 5.0), (10.0, 0.0)], &[]);
        polyline.vertices[0].bulge = 0.3;
        let output = delete_vertex(&polyline, 1).unwrap();
        assert_eq!(output.vertices.len(), 2);
        assert_eq!(output.vertices[0].bulge, 0.0);
        assert!(delete_vertex(&output, 0).is_err());
        let mut ring = line3d(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)], &[]);
        ring.closed = true;
        let three = delete_vertices(&ring, vec![3]).unwrap();
        assert!(delete_vertex(&three, 0).is_err());
        assert!(delete_vertices(&ring, vec![0, 1]).is_err());
        let first = delete_vertex(&ring, 0).unwrap();
        assert_eq!(first.vertices[0].position, point(10.0, 0.0));
    }
    #[test]
    fn moving_a_vertex_keeps_each_arcs_bulge() {
        let mut polyline = line3d(
            &[(0.0, 0.0), (2.0, 0.0), (4.0, 0.0)],
            &[Some(1.0), Some(2.0), Some(3.0)],
        );
        polyline.vertices[0].bulge = 0.5;
        let output = move_vertex(&polyline, 1, point(3.0, 1.0)).unwrap();
        assert_eq!(output.vertices[1].position, point(3.0, 1.0));
        assert_eq!(output.vertices[0].bulge, 0.5);
        assert_eq!(output.vertices[1].height, Some(2.0));
        let moved = move_segment(&polyline, 1, Vec2::new(0.0, 2.0)).unwrap();
        assert_eq!(moved.vertices[1].position, point(2.0, 2.0));
        assert_eq!(moved.vertices[2].position, point(4.0, 2.0));
        assert_eq!(moved.vertices[0].position, point(0.0, 0.0));
    }
    #[test]
    fn segments_become_arcs_and_lines() {
        let polyline = line3d(&[(0.0, 0.0), (2.0, 0.0)], &[]);
        let arc = segment_to_arc(&polyline, 0, point(1.0, 1.0)).unwrap();
        near(arc.vertices[0].bulge, -1.0, 1e-12);
        assert!(segment_to_arc(&polyline, 0, point(1.0, 0.0)).is_err());
        let back = segment_to_line(&arc, 0).unwrap();
        assert_eq!(back.vertices[0].bulge, 0.0);
        assert!(segment_to_line(&back, 0).is_err());
    }
    #[test]
    fn straighten_removes_the_vertices_between() {
        let polyline = line3d(
            &[(0.0, 0.0), (1.0, 1.0), (2.0, -1.0), (3.0, 1.0), (4.0, 0.0)],
            &[],
        );
        let output = straighten(&polyline, 3, 0).unwrap();
        assert_eq!(output.vertices.len(), 3);
        assert_eq!(output.vertices[0].position, point(0.0, 0.0));
        assert_eq!(output.vertices[1].position, point(3.0, 1.0));
        assert_eq!(vertices_between(&polyline, 1, 4), vec![2, 3]);
        let mut ring = line3d(
            &[
                (0.0, 0.0),
                (10.0, 0.0),
                (10.0, 10.0),
                (5.0, 12.0),
                (0.0, 10.0),
            ],
            &[],
        );
        ring.closed = true;
        assert_eq!(vertices_between(&ring, 3, 1), vec![4, 0]);
        let cut = straighten(&ring, 3, 1).unwrap();
        assert_eq!(cut.vertices.len(), 3);
        assert!(straighten(&ring, 2, 2).is_err());
    }
    #[test]
    fn grade_and_interpolate_set_heights_by_length() {
        let polyline = line3d(
            &[(0.0, 0.0), (3.0, 0.0), (3.0, 4.0), (10.0, 4.0)],
            &[Some(10.0), None, Some(50.0), None],
        );
        let mut ends = polyline.clone();
        ends.vertices[3].height = Some(24.0);
        let graded = grade_between(&ends, 0, 3).unwrap();
        near(graded.vertices[1].height.unwrap(), 13.0, 1e-12);
        near(graded.vertices[2].height.unwrap(), 17.0, 1e-12);
        assert!(grade_between(&polyline, 0, 3).is_err());
        let filled = interpolate_heights(&polyline).unwrap();
        near(
            filled.vertices[1].height.unwrap(),
            10.0 + 40.0 * 3.0 / 7.0,
            1e-12,
        );
        assert!(filled.vertices[3].height.is_none());
    }
    #[test]
    fn weed_drops_what_lies_within_the_tolerance() {
        let polyline = line3d(
            &[
                (0.0, 0.0),
                (1.0, 0.02),
                (2.0, -0.01),
                (3.0, 0.0),
                (3.0, 5.0),
            ],
            &[],
        );
        let output = weed(&polyline, 0.05, &[]).unwrap();
        assert_eq!(output.vertices.len(), 3);
        assert_eq!(output.vertices[1].position, point(3.0, 0.0));
        assert_eq!(weed(&polyline, 0.005, &[]).unwrap().vertices.len(), 5);
        let kept = weed(&polyline, 0.05, &[false, true, false, false, false]).unwrap();
        assert_eq!(kept.vertices.len(), 4);
    }
    #[test]
    fn weed_counts_height_and_keeps_arcs() {
        let polyline = line3d(
            &[(0.0, 0.0), (5.0, 0.0), (10.0, 0.0)],
            &[Some(0.0), Some(1.0), Some(0.0)],
        );
        assert_eq!(weed(&polyline, 0.1, &[]).unwrap().vertices.len(), 3);
        assert_eq!(weed(&polyline, 2.0, &[]).unwrap().vertices.len(), 2);
        let mut arc = line3d(&[(0.0, 0.0), (5.0, 0.0), (10.0, 0.0), (15.0, 0.0)], &[]);
        arc.vertices[1].bulge = 0.2;
        assert_eq!(weed(&arc, 100.0, &[]).unwrap().vertices.len(), 4);
        let mut ring = line3d(
            &[
                (0.0, 0.0),
                (5.0, 0.001),
                (10.0, 0.0),
                (10.0, 10.0),
                (0.0, 10.0),
            ],
            &[],
        );
        ring.closed = true;
        let square = weed(&ring, 0.01, &[]).unwrap();
        assert_eq!(square.vertices.len(), 4);
        assert!(weed(&ring, 1000.0, &[]).unwrap().vertices.len() >= 3);
    }
    #[test]
    fn densify_divides_straights_and_chords_or_splits_arcs() {
        let polyline = line3d(&[(0.0, 0.0), (10.0, 0.0)], &[Some(0.0), Some(10.0)]);
        let output = densify(&polyline, 3.0, 0.0).unwrap();
        assert_eq!(output.vertices.len(), 5);
        expect_near(output.vertices[1].position, point(2.5, 0.0));
        near(output.vertices[2].height.unwrap(), 5.0, 1e-12);
        let mut arc = line3d(&[(0.0, 0.0), (2.0, 0.0)], &[]);
        arc.vertices[0].bulge = 1.0;
        let split = densify(&arc, 1.0, 0.0).unwrap();
        assert_eq!(split.vertices.len(), 5);
        near(split.length(), PI, 1e-9);
        let chorded = densify(&arc, 0.0, 0.01).unwrap();
        assert!(!chorded.has_arcs());
        for vertex in &chorded.vertices {
            near(vertex.position.distance_to(point(1.0, 0.0)), 1.0, 1e-9);
        }
        assert!(densify(&polyline, 0.0, 0.0).is_err());
    }
    #[test]
    fn merge_snap_close_open_and_start() {
        let polyline = line3d(&[(0.0, 0.0), (0.001, 0.0), (5.0, 0.0), (5.0, 5.0)], &[]);
        let merged = merge_near_vertices(&polyline, 0.01).unwrap();
        assert_eq!(merged.vertices.len(), 3);
        let snapped = snap_to_grid(
            &line3d(&[(0.4, 0.6), (2.2, 2.9)], &[]),
            1.0,
            Point2::default(),
        )
        .unwrap();
        assert_eq!(snapped.vertices[0].position, point(0.0, 1.0));
        assert_eq!(snapped.vertices[1].position, point(2.0, 3.0));
        let closed = close_polyline(&polyline).unwrap();
        assert!(closed.closed);
        assert!(close_polyline(&closed).is_err());
        let reopened = open_polyline(&closed).unwrap();
        assert!(!reopened.closed);
        let on_start = close_polyline(&line3d(
            &[(0.0, 0.0), (4.0, 0.0), (4.0, 4.0), (0.0, 0.0)],
            &[],
        ))
        .unwrap();
        assert_eq!(on_start.vertices.len(), 3);
        let rotated = change_start_vertex(&closed, 2).unwrap();
        assert_eq!(rotated.vertices[0].position, point(5.0, 0.0));
        near(rotated.area(), closed.area(), 1e-12);
        assert!(change_start_vertex(&polyline, 1).is_err());
    }
    #[test]
    fn fillet_rounds_a_corner_with_a_tangent_arc() {
        let polyline = line3d(
            &[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)],
            &[Some(0.0), Some(10.0), Some(20.0)],
        );
        let output = fillet_vertex(&polyline, 1, 2.0).unwrap();
        assert_eq!(output.vertices.len(), 4);
        expect_near(output.vertices[1].position, point(8.0, 0.0));
        expect_near(output.vertices[2].position, point(10.0, 2.0));
        near(output.vertices[1].bulge, (PI / 8.0).tan(), 1e-12);
        let CurveSegment::Arc(arc) = output.segment(1) else {
            panic!("expected arc")
        };
        expect_near(arc.center, point(8.0, 2.0));
        near(arc.radius, 2.0, 1e-12);
        near(output.vertices[1].height.unwrap(), 8.0, 1e-12);
        near(output.vertices[2].height.unwrap(), 12.0, 1e-12);
        assert!(fillet_vertex(&polyline, 1, 20.0).is_err());
        assert!(fillet_vertex(&polyline, 0, 1.0).is_err());
        assert!(
            fillet_vertex(&line3d(&[(0.0, 0.0), (1.0, 0.0), (2.0, 0.0)], &[]), 1, 0.1).is_err()
        );
    }
    #[test]
    fn check_corner_refuses_what_no_fillet_or_chamfer_could_cut() {
        let corner = line3d(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)], &[]);
        assert!(check_corner(&corner, 1).is_ok());
        let end = check_corner(&corner, 0).unwrap_err();
        assert_eq!(
            end.message,
            "vertex 0 is an end of the polyline; a corner is where two segments meet"
        );
        assert!(check_corner(&corner, 2).is_err());
        assert!(check_corner(&corner, 3).is_err());
        let square = CurvePolyline2::from_points(
            &[
                point(0.0, 0.0),
                point(10.0, 0.0),
                point(10.0, 10.0),
                point(0.0, 10.0),
            ],
            true,
        );
        assert!(check_corner(&square, 0).is_ok());
        let mut bent = corner;
        bent.vertices[0].bulge = 0.5;
        assert!(
            check_corner(&bent, 1)
                .unwrap_err()
                .message
                .contains("beside an arc")
        );
        assert!(
            check_corner(&line3d(&[(0.0, 0.0), (1.0, 0.0), (2.0, 0.0)], &[]), 1)
                .unwrap_err()
                .message
                .contains("in line")
        );
        assert!(
            check_corner(&line3d(&[(0.0, 0.0), (0.0, 0.0), (0.0, 10.0)], &[]), 1)
                .unwrap_err()
                .message
                .contains("has no length")
        );
    }
    #[test]
    fn chamfer_cuts_a_corner() {
        let polyline = line3d(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)], &[]);
        let output = chamfer_vertex(&polyline, 1, 2.0, 3.0).unwrap();
        assert_eq!(output.vertices.len(), 4);
        expect_near(output.vertices[1].position, point(8.0, 0.0));
        expect_near(output.vertices[2].position, point(10.0, 3.0));
        assert_eq!(output.vertices[1].bulge, 0.0);
        let whole = chamfer_vertex(&polyline, 1, 10.0, 3.0).unwrap();
        assert_eq!(whole.vertices.len(), 3);
    }
    #[test]
    fn nearest_vertex_and_segment() {
        let polyline = line3d(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)], &[]);
        assert_eq!(nearest_vertex(&polyline, point(9.0, 1.0)), Some(1));
        assert_eq!(nearest_segment(&polyline, point(11.0, 6.0)), Some(1));
        assert_eq!(minimum_vertices(&polyline), 2);
    }
    #[test]
    fn sub_path_cuts_arcs_on_their_circle_and_interpolates_heights() {
        let mut polyline = CurvePolyline2::from_points(
            &[point(0.0, 0.0), point(10.0, 0.0), point(20.0, 0.0)],
            false,
        );
        polyline.vertices[1].bulge = 1.0;
        polyline.vertices[0].height = Some(0.0);
        polyline.vertices[1].height = Some(10.0);
        polyline.vertices[2].height = Some(20.0);
        let arc_length = 5.0 * PI;
        let part = sub_path(&polyline, 5.0, 10.0 + arc_length / 2.0);
        assert_eq!(part.vertices.len(), 3);
        assert_eq!(part.vertices[0].position, point(5.0, 0.0));
        double_eq(part.vertices[0].height.unwrap(), 5.0);
        assert_eq!(part.vertices[1].position, point(10.0, 0.0));
        double_eq(part.vertices[1].height.unwrap(), 10.0);
        near(part.vertices[1].bulge, (PI / 8.0).tan(), 1e-12);
        near(
            part.vertices[2].position.distance_to(point(15.0, -5.0)),
            0.0,
            1e-9,
        );
        near(part.vertices[2].height.unwrap(), 15.0, 1e-9);
        near(part.length(), 5.0 + arc_length / 2.0, 1e-9);
    }
    #[test]
    fn wrapping_path_runs_round_through_the_first_vertex() {
        let square = CurvePolyline2::from_points(
            &[
                point(0.0, 0.0),
                point(4.0, 0.0),
                point(4.0, 4.0),
                point(0.0, 4.0),
            ],
            true,
        );
        let part = wrapping_path(&square, 14.0, 2.0);
        assert_eq!(part.vertices.len(), 3);
        assert_eq!(part.vertices[0].position, point(0.0, 2.0));
        assert_eq!(part.vertices[1].position, point(0.0, 0.0));
        assert_eq!(part.vertices.last().unwrap().position, point(2.0, 0.0));
        assert!(!part.closed);
    }
}
