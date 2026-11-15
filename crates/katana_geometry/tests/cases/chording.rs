mod chording {
    use crate::support::near;
    use katana_geometry::chording::*;
    use katana_geometry::{Arc2, Circle2, Point2};
    use katana_math::numerics::TWO_PI;
    #[test]
    fn chording_an_arc_honours_its_sagitta_tolerance() {
        let arc = Arc2::new(Point2::new(0.0, 0.0), 50.0, 0.0, TWO_PI * 0.25);
        for epsilon in [1.0, 0.1, 0.01, 0.001] {
            let points = chord_arc(&arc, epsilon);
            assert!(points.len() >= 2);
            assert_eq!(points[0], arc.start_point());
            near(
                points.last().unwrap().distance_to(arc.end_point()),
                0.0,
                1e-9,
            );
            for pair in points.windows(2) {
                let middle = (pair[0] + pair[1]) * 0.5;
                let deviation = (middle.distance_to(arc.center) - arc.radius).abs();
                assert!(deviation <= epsilon * 1.001);
            }
            assert!(points.len() <= 8193);
        }
        assert!(chord_arc(&arc, 0.001).len() > chord_arc(&arc, 0.1).len());
    }
    #[test]
    fn chording_a_circle_does_not_repeat_the_closing_point() {
        let circle = Circle2::new(Point2::new(10.0, 20.0), 5.0);
        let points = chord_circle(&circle, 0.01);
        assert!(points.len() >= 3);
        assert!(points[0].distance_to(*points.last().unwrap()) > 1e-6);
        for point in points {
            near(point.distance_to(circle.center), circle.radius, 1e-9);
        }
    }
    #[test]
    fn chording_a_degenerate_arc_does_not_hang() {
        assert!(chord_arc(&Arc2::new(Point2::new(0.0, 0.0), 0.0, 0.0, 1.0), 0.01).is_empty());
        assert!(chord_arc(&Arc2::new(Point2::new(0.0, 0.0), 10.0, 0.0, 0.0), 0.01).len() <= 2);
        assert!(chord_arc(&Arc2::new(Point2::new(0.0, 0.0), 10.0, 0.0, TWO_PI), 1e9).len() >= 2);
        assert!(
            chord_arc(&Arc2::new(Point2::new(0.0, 0.0), 1e9, 0.0, TWO_PI), 1e-12).len() <= 8193
        );
    }
    #[test]
    fn the_count_rule_is_the_one_the_arc_and_the_spiral_both_use() {
        let arc = Arc2::new(Point2::new(0.0, 0.0), 300.0, 0.0, 0.3);
        let count = sagitta_chord_count(300.0, 0.3, 0.001);
        assert_eq!(chord_arc(&arc, 0.001).len(), count + 1);
        assert_eq!(count, 59);
    }
}
