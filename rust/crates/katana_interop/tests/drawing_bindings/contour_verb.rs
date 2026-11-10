//! tests/geo/test_contour_verb.cpp: the ContourEntities suite, the one place
//! CONTOUR's two engines meet (katana_interop::geo::contour_entities). The
//! file's ContourContract case binds GDAL's arguments (katana_gis's
//! processing target); its other suites drive the verb through the app layer
//! (step 6).

mod contour_entities {
    use katana_core::ErrorCode;
    use katana_entity::Model;
    use katana_geometry::{Point2, Polyline2};
    use katana_gis::processing::{
        Feature, FeatureSet, FeatureTable, FieldDef, FieldType, FieldValue,
    };
    use katana_gis::{GeoPoint, GeometryKind, VectorGeometry};
    use katana_interop::geo::contour_entities::{
        ContourBoundary, ContourEntityOptions, clip_contours, contour_command,
        contours_from_features,
    };
    use katana_terrain::Contour;

    use crate::{double_eq, must, near, point};

    fn line(vertices: &[(f64, f64)], closed: bool) -> Polyline2 {
        Polyline2::new(
            vertices.iter().map(|&(x, y)| Point2::new(x, y)).collect(),
            closed,
        )
    }

    #[test]
    fn gdals_lines_become_contours_at_their_levels_rings_closed() {
        let feature = |points: Vec<GeoPoint>, level: f64| Feature {
            parts: vec![VectorGeometry {
                kind: GeometryKind::LineString,
                parts: vec![points],
                ..VectorGeometry::default()
            }],
            values: vec![FieldValue::Integer64(0), FieldValue::Real(level)],
        };
        let table = FeatureTable {
            name: String::from("contour"),
            kind: GeometryKind::LineString,
            fields: vec![
                FieldDef {
                    name: String::from("ID"),
                    r#type: FieldType::Integer64,
                },
                FieldDef {
                    name: String::from("elevation"),
                    r#type: FieldType::Real,
                },
            ],
            features: vec![
                feature(
                    vec![
                        point(0.0, 0.0, 3.0),
                        point(1.0, 0.0, 3.0),
                        point(1.0, 1.0, 3.0),
                        point(0.0, 0.0, 3.0),
                    ],
                    3.0,
                ),
                feature(vec![point(0.0, 5.0, 2.0), point(9.0, 5.0, 2.0)], 2.0),
            ],
            ..FeatureTable::default()
        };
        let set = FeatureSet {
            tables: vec![table],
        };
        let contours = must(contours_from_features(&set, "elevation", 1.0, 0.0, 3));
        assert_eq!(contours.len(), 2);
        double_eq(contours[0].elevation, 2.0); // sorted by level
        assert!(!contours[0].line.closed);
        assert!(!contours[0].major);
        double_eq(contours[1].elevation, 3.0);
        assert!(contours[1].line.closed);
        assert_eq!(contours[1].line.vertices.len(), 3); // the repeat dropped
        assert!(contours[1].major); // k = 3, a multiple of 3

        let error = contours_from_features(&set, "level", 1.0, 0.0, 3)
            .expect_err("a table without the field");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
    }

    #[test]
    fn a_ring_cut_by_a_boundary_is_one_piece_joined_across_its_start() {
        // A 10 x 10 square ring starting at (0,0), cut by the half-plane box
        // x <= 5 (a 20 x 20 box from (-5,-5) to (5,15)): what is inside runs
        // from (5,10) along the top to (0,10), down to (0,0) - the ring's
        // start - and along the bottom to (5,0). One piece of 5 + 10 + 5 = 20.
        let ring = Contour {
            elevation: 7.0,
            major: false,
            line: line(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)], true),
        };
        let bounds = ContourBoundary {
            rings: vec![line(
                &[(-5.0, -5.0), (5.0, -5.0), (5.0, 15.0), (-5.0, 15.0)],
                true,
            )],
        };
        let kept = clip_contours(std::slice::from_ref(&ring), &[bounds]);
        assert_eq!(kept.len(), 1);
        assert!(!kept[0].line.closed);
        near(kept[0].line.length(), 20.0, 1e-12);
        assert_eq!(kept[0].line.vertices[0], Point2::new(5.0, 10.0));
        assert_eq!(
            kept[0].line.vertices[kept[0].line.vertices.len() - 1],
            Point2::new(5.0, 0.0)
        );
        double_eq(kept[0].elevation, 7.0);

        // A ring wholly inside stays a ring.
        let all = line(
            &[(-1.0, -1.0), (11.0, -1.0), (11.0, 11.0), (-1.0, 11.0)],
            true,
        );
        let whole = clip_contours(
            std::slice::from_ref(&ring),
            &[ContourBoundary { rings: vec![all] }],
        );
        assert_eq!(whole.len(), 1);
        assert!(whole[0].line.closed);
    }

    #[test]
    fn a_hole_in_a_boundary_cuts_the_contour_out() {
        // A line along y = 5 from x = 0 to 20 through an area from 0 to 20
        // with a hole from 5 to 15: kept 0 to 5 and 15 to 20.
        let contour = Contour {
            elevation: 1.0,
            major: false,
            line: line(&[(0.0, 5.0), (20.0, 5.0)], false),
        };
        let outer = line(&[(0.0, 0.0), (20.0, 0.0), (20.0, 10.0), (0.0, 10.0)], true);
        let hole = line(&[(5.0, 2.0), (15.0, 2.0), (15.0, 8.0), (5.0, 8.0)], true);
        let kept = clip_contours(
            &[contour],
            &[ContourBoundary {
                rings: vec![outer, hole],
            }],
        );
        assert_eq!(kept.len(), 2);
        near(kept[0].line.length(), 5.0, 1e-12);
        near(kept[1].line.length(), 5.0, 1e-12);
        assert_eq!(kept[1].line.vertices[0], Point2::new(15.0, 5.0));
    }

    #[test]
    fn no_contours_make_no_command_and_a_bad_layer_is_refused() {
        let model = Model::default();
        let none = must(contour_command(
            &model,
            &[],
            &ContourEntityOptions::default(),
        ));
        assert!(none.command.is_none());
        let one = Contour {
            line: line(&[(0.0, 0.0), (1.0, 1.0)], false),
            ..Contour::default()
        };
        let options = ContourEntityOptions {
            layer: String::new(),
            ..ContourEntityOptions::default()
        };
        let Err(error) = contour_command(&model, &[one], &options) else {
            panic!("an empty layer should have been refused");
        };
        assert_eq!(error.code, ErrorCode::InvalidArgument);
    }
}
