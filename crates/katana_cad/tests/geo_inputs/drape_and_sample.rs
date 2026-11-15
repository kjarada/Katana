//! tests/geo/test_drape_and_sample.cpp: the Drape suite, the drape's two
//! halves (katana_cad::geo::drape; docs/terrain.md "Sampling and drape").

mod drape {
    use std::sync::Arc;

    use katana_cad::Document;
    use katana_cad::geo::drape::{HeightAt, drape_command, drape_heights};
    use katana_commands::create_entities;
    use katana_entity::entity::heights_of;
    use katana_entity::{Entity, Geometry};
    use katana_geometry::{Point2, Polyline2};

    use crate::double_eq;

    #[test]
    fn heights_are_computed_first_and_written_as_one_command() {
        let mut document = Document::default();
        let entity = Entity {
            geometry: Geometry::Polyline(Polyline2 {
                vertices: vec![
                    Point2::new(0.0, 0.0),
                    Point2::new(10.0, 0.0),
                    Point2::new(20.0, 0.0),
                ],
                closed: false,
            }),
            ..Entity::default()
        };
        let status = document.execute(create_entities(vec![entity]));
        assert!(status.is_ok(), "{status:?}");
        let id = document.last_created_entities()[0];
        let copy = document
            .model()
            .entities
            .find(id)
            .expect("the polyline is drawn")
            .clone();
        // Ground rising 1 in 10 east, and nothing beyond x = 15.
        let ground: HeightAt = Arc::new(|at: Point2| {
            if at.x <= 15.0 {
                Some(at.x / 10.0)
            } else {
                None
            }
        });
        let heights = drape_heights(&[copy], &*ground, None);
        assert_eq!(heights.entities.len(), 1);
        assert_eq!(heights.vertices, 3);
        assert_eq!(heights.vertices_off, 1);
        let made = drape_command(document.model(), &heights.entities, "DRAPE test");
        assert_eq!(made.changed, 1);
        let command = made.command.expect("the drape changes the polyline");
        let status = document.execute(command);
        assert!(status.is_ok(), "{status:?}");
        let z = heights_of(
            &document
                .model()
                .entities
                .find(id)
                .expect("the polyline is still drawn")
                .properties,
            3,
        );
        double_eq(z[0].unwrap_or(-1.0), 0.0);
        double_eq(z[1].unwrap_or(-1.0), 1.0);
        assert!(z[2].is_none());
    }
}
