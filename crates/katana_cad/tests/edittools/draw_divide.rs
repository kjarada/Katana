// Divide and Measure (src/katana_cad/tools/draw_divide.rs;
// tests/cad/tools/test_draw_divide.cpp): points along an object, checked
// against positions worked out by hand. Lengths are chosen so the stations are
// exact in binary (quarters of 10, halves of 16) wherever the object is
// straight; round a circle the positions go through cos and sin and are
// compared to 1e-12.

use katana_cad::interactive_tool::ToolInput;
use katana_commands::{
    EntityAttributes, create_circle, create_entities, create_line, create_polyline, create_style,
    create_text,
};
use katana_entity::entity::{heights_of, set_heights};
use katana_entity::{Entity, Geometry, Style, TextGeometry};
use katana_geometry::{Point2, Polyline2};

use crate::tool_driver::ToolDriver;
use crate::{CONTINUE, DONE, REJECTED, double_eq, near, ok};

fn p(x: f64, y: f64) -> Point2 {
    Point2::new(x, y)
}

// Every point entity in the drawing, in id order - the order they were made
// in.
fn points(driver: &ToolDriver) -> Vec<Entity> {
    let mut out = Vec::new();
    driver.document().model().entities.for_each(|entity| {
        if matches!(entity.geometry, Geometry::Point(_)) {
            out.push(entity.clone());
        }
    });
    out.sort_by_key(|entity| entity.id);
    out
}

#[track_caller]
fn at(point: &Entity) -> Point2 {
    match &point.geometry {
        Geometry::Point(geometry) => geometry.position,
        other => panic!("not a point: {other:?}"),
    }
}

mod draw_divide {
    use super::*;

    #[test]
    fn a_line_in_four_parts_gets_three_points_at_its_quarters() {
        let mut driver = ToolDriver::new();
        let line = driver.add(create_line(
            p(0.0, 0.0),
            p(10.0, 0.0),
            EntityAttributes::default(),
        ));
        driver.start("draw.divide");
        assert_eq!(driver.expects(), ToolInput::Entity);
        assert_eq!(driver.pick(line, 5.0, 0.0).outcome, CONTINUE);
        assert_eq!(driver.r#type("1").outcome, REJECTED); // one part divides nothing
        let step = driver.r#type("4");
        assert_eq!(step.outcome, DONE);
        assert_eq!(step.message, "Divided into 4 parts: 3 points");
        // 10 / 4 = 2.5: the ends are marked already, so 2.5, 5 and 7.5.
        let made = points(&driver);
        assert_eq!(made.len(), 3);
        assert_eq!(at(&made[0]), p(2.5, 0.0));
        assert_eq!(at(&made[1]), p(5.0, 0.0));
        assert_eq!(at(&made[2]), p(7.5, 0.0));
        // One command: one undo takes all three.
        ok(driver.document_mut().undo());
        assert!(points(&driver).is_empty());
    }

    #[test]
    fn a_closed_square_in_eight_parts_gets_a_point_at_every_division_including_the_start() {
        let mut driver = ToolDriver::new();
        let square = driver.add(create_polyline(
            Polyline2::new(
                vec![p(0.0, 0.0), p(4.0, 0.0), p(4.0, 4.0), p(0.0, 4.0)],
                true,
            ),
            EntityAttributes::default(),
        ));
        driver.start("draw.divide");
        let _ = driver.pick(square, 2.0, 0.0);
        assert_eq!(driver.r#type("8").outcome, DONE);
        // Perimeter 16, parts of 2, from the first vertex round the way it
        // runs.
        let expected = [
            p(0.0, 0.0),
            p(2.0, 0.0),
            p(4.0, 0.0),
            p(4.0, 2.0),
            p(4.0, 4.0),
            p(2.0, 4.0),
            p(0.0, 4.0),
            p(0.0, 2.0),
        ];
        let made = points(&driver);
        assert_eq!(made.len(), expected.len());
        for (i, (point, wanted)) in made.iter().zip(&expected).enumerate() {
            assert_eq!(at(point), *wanted, "point {i}");
        }
    }

    #[test]
    fn a_circle_in_four_parts_gets_its_quadrants_from_east_anticlockwise() {
        let mut driver = ToolDriver::new();
        let circle = driver.add(create_circle(p(0.0, 0.0), 1.0, EntityAttributes::default()));
        driver.start("draw.divide");
        let _ = driver.pick(circle, 1.0, 0.0);
        assert_eq!(driver.r#type("4").outcome, DONE);
        let expected = [p(1.0, 0.0), p(0.0, 1.0), p(-1.0, 0.0), p(0.0, -1.0)];
        let made = points(&driver);
        assert_eq!(made.len(), 4);
        for (point, wanted) in made.iter().zip(&expected) {
            near(at(point).x, wanted.x, 1e-12);
            near(at(point).y, wanted.y, 1e-12);
        }
    }

    #[test]
    fn points_along_a3_d_string_get_the_height_interpolated_there() {
        let mut driver = ToolDriver::new();
        let mut string = Entity {
            geometry: Geometry::Polyline(Polyline2::new(vec![p(0.0, 0.0), p(10.0, 0.0)], false)),
            ..Entity::default()
        };
        set_heights(&mut string.properties, &[Some(100.0), Some(110.0)]);
        let id = driver.add(create_entities(vec![string]));
        driver.start("draw.divide");
        let _ = driver.pick(id, 5.0, 0.0);
        assert_eq!(driver.r#type("2").outcome, DONE);
        // The midpoint, half way from 100 to 110.
        let made = points(&driver);
        assert_eq!(made.len(), 1);
        assert_eq!(at(&made[0]), p(5.0, 0.0));
        let height = heights_of(&made[0].properties, 1)[0];
        let Some(height) = height else {
            panic!("the point has no height");
        };
        double_eq(height, 105.0);
    }

    #[test]
    fn the_style_option_puts_the_points_in_a_named_style_and_refuses_an_unknown_one() {
        let mut driver = ToolDriver::new();
        let post = Style {
            name: String::from("POST"),
            ..Style::default()
        };
        ok(driver.document_mut().execute(create_style(post)));
        let line = driver.add(create_line(
            p(0.0, 0.0),
            p(10.0, 0.0),
            EntityAttributes::default(),
        ));
        driver.start("draw.divide");
        let _ = driver.pick(line, 5.0, 0.0);
        assert_eq!(driver.r#type("S").outcome, CONTINUE);
        assert_eq!(driver.r#type("NO SUCH STYLE").outcome, REJECTED);
        assert_eq!(driver.r#type("POST").outcome, CONTINUE);
        assert_eq!(driver.r#type("2").outcome, DONE);
        let made = points(&driver);
        assert_eq!(made.len(), 1);
        assert_eq!(made[0].style, "POST");
    }

    #[test]
    fn an_object_with_no_length_is_refused_with_its_kind() {
        let mut driver = ToolDriver::new();
        let text = driver.add(create_text(
            TextGeometry {
                position: p(0.0, 0.0),
                text: String::from("LOT 42"),
                height: 2.5,
                rotation: 0.0,
                ..TextGeometry::default()
            },
            EntityAttributes::default(),
        ));
        driver.start("draw.divide");
        let step = driver.pick(text, 0.0, 0.0);
        assert_eq!(step.outcome, REJECTED);
        assert_eq!(step.message, "a text has no length to place points along");
    }
}

mod draw_measure {
    use super::*;

    #[test]
    fn every_three_along_a_ten_line_goes_from_the_end_nearer_the_pick() {
        let mut driver = ToolDriver::new();
        let line = driver.add(create_line(
            p(0.0, 0.0),
            p(10.0, 0.0),
            EntityAttributes::default(),
        ));
        driver.start("draw.measure");
        let _ = driver.pick(line, 1.0, 0.0); // nearer the start
        assert_eq!(driver.r#type("3").outcome, DONE);
        // 3, 6, 9; 12 is past the end.
        let made = points(&driver);
        assert_eq!(made.len(), 3);
        assert_eq!(at(&made[0]), p(3.0, 0.0));
        assert_eq!(at(&made[2]), p(9.0, 0.0));

        ok(driver.document_mut().undo());
        driver.start("draw.measure");
        let _ = driver.pick(line, 9.0, 0.0); // nearer the end: 10 - 3, 10 - 6, 10 - 9
        assert_eq!(driver.r#type("3").outcome, DONE);
        let made = points(&driver);
        assert_eq!(made.len(), 3);
        assert_eq!(at(&made[0]), p(7.0, 0.0));
        assert_eq!(at(&made[1]), p(4.0, 0.0));
        assert_eq!(at(&made[2]), p(1.0, 0.0));
    }

    #[test]
    fn a_length_that_divides_the_object_exactly_puts_no_point_on_its_end() {
        let mut driver = ToolDriver::new();
        let line = driver.add(create_line(
            p(0.0, 0.0),
            p(10.0, 0.0),
            EntityAttributes::default(),
        ));
        driver.start("draw.measure");
        let _ = driver.pick(line, 0.0, 0.0);
        assert_eq!(driver.r#type("2.5").outcome, DONE);
        // 2.5, 5, 7.5 - and not 10, the end.
        let made = points(&driver);
        assert_eq!(made.len(), 3);
        assert_eq!(at(&made[2]), p(7.5, 0.0));
    }
}
