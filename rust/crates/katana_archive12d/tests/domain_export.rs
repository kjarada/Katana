//! The twin of tests/archive12d/test_domain_export.cpp: a drawing out to an
//! archive and back, and what the archive is told about its styles, colours
//! and surfaces.

#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

pub mod support;

use std::cmp::Ordering;
use std::f64::consts::PI;

use katana_archive12d::{
    AttributeValue, DomainExport, DomainImport, Element, ExportOptions, ExportSurface,
    ImportOptions, ReadOptions, Tin, VertexString, WriteOptions, from_domain, read_archive,
    to_domain, write_archive,
};
use katana_entity::entity::INVALID_ENTITY_ID;
use katana_entity::{
    Alignment, Color, DimensionGeometry, Entity, EntityId, Geometry, Layer, Model, PointGeometry,
    PropertyValue, Style, TextGeometry,
};
use katana_geometry::{
    AlignmentPi, Arc2, Circle2, Point2, Point3, Polyline2, ProfilePvi, Segment2, Vec2,
    VerticalAlignment,
};
use katana_terrain::TinSurface;

use support::{get, near, prop_integer, prop_real, prop_text, shape};

fn make(geometry: Geometry, layer: &str) -> Entity {
    Entity {
        geometry,
        layer: String::from(layer),
        ..Entity::default()
    }
}

fn add(model: &mut Model, entity: Entity) -> EntityId {
    match model.entities.add(entity) {
        Ok(id) => id,
        Err(error) => panic!("{}", error.describe()),
    }
}

fn add_geometry(model: &mut Model, geometry: impl Into<Geometry>, layer: &str) -> EntityId {
    add(model, make(geometry.into(), layer))
}

fn export(model: &Model, surfaces: &[ExportSurface<'_>]) -> DomainExport {
    from_domain(model, surfaces, &ExportOptions::default(), &[])
        .unwrap_or_else(|error| panic!("{}", error.describe()))
}

// Model -> archive -> .12da text -> archive -> domain: everything a user
// would put a drawing through to get it into the archive format and back.
fn round_trip(model: &Model, surfaces: &[ExportSurface<'_>]) -> (DomainImport, DomainExport) {
    let exported = export(model, surfaces);
    let text = write_archive(&exported.archive, &WriteOptions::default());
    let archive = read_archive(text.as_bytes(), &ReadOptions::default())
        .unwrap_or_else(|error| panic!("{}\n{text}", error.describe()));
    let back = to_domain(&archive, &ImportOptions::default())
        .unwrap_or_else(|error| panic!("{}", error.describe()));
    (back, exported)
}

fn polyline(points: &[(f64, f64)], closed: bool) -> Polyline2 {
    Polyline2::new(
        points.iter().map(|&(x, y)| Point2::new(x, y)).collect(),
        closed,
    )
}

// An import, installed as the application installs it, with every style
// renamed on the way in - which is what renaming a style in Katana does: the
// style takes the new name and every entity drawn with it follows.
fn imported_with_styles_renamed(text: &str) -> Model {
    let archive = read_archive(text, &ReadOptions::default())
        .unwrap_or_else(|error| panic!("{}", error.describe()));
    let domain = to_domain(&archive, &ImportOptions::default())
        .unwrap_or_else(|error| panic!("{}", error.describe()));
    let renamed = |name: &str| {
        if name.is_empty() {
            String::new()
        } else {
            format!("Katana {name}")
        }
    };
    let mut model = Model::default();
    for layer in &domain.layers_needed {
        assert!(model.layers.add(layer.clone()).is_ok());
    }
    for style in &domain.styles_needed {
        let mut style = style.clone();
        style.name = renamed(style.name.as_str());
        assert!(model.styles.add(style).is_ok());
    }
    for entity in &domain.entities {
        let mut entity = entity.clone();
        entity.style = renamed(entity.style.as_str());
        add(&mut model, entity);
    }
    model
}

mod domain_export {
    use super::*;

    #[test]
    fn every_drawable_entity_comes_back_as_the_geometry_it_was() {
        let mut model = Model::default();
        assert!(
            model
                .layers
                .add(Layer {
                    name: String::from("survey/kerb"),
                    ..Layer::default()
                })
                .is_ok()
        );
        add_geometry(
            &mut model,
            PointGeometry {
                position: Point2::new(1.5, 2.5),
            },
            "survey/kerb",
        );
        add_geometry(
            &mut model,
            Segment2::new(Point2::new(0.0, 0.0), Point2::new(3.0, 4.0)),
            "0",
        );
        add_geometry(
            &mut model,
            polyline(&[(0.0, 0.0), (10.0, 0.0), (10.0, 5.0)], true),
            "0",
        );
        add_geometry(
            &mut model,
            Circle2::new(Point2::new(650.0, 200.0), 8.0),
            "0",
        );
        add_geometry(
            &mut model,
            TextGeometry::new(Point2::new(5.0, 6.0), "RL \"32.45\"", 2.75, PI / 2.0),
            "0",
        );

        let (back, exported) = round_trip(&model, &[]);
        assert_eq!(exported.entities_written, 5);
        assert_eq!(exported.entities_skipped, 0);
        assert_eq!(back.entities.len(), 5);
        assert_eq!(
            back.entities[0].layer, "survey/kerb",
            "a layer path is an archive tree name"
        );
        assert_eq!(
            shape::<PointGeometry>(&back.entities[0].geometry).position,
            Point2::new(1.5, 2.5)
        );
        assert_eq!(
            shape::<Segment2>(&back.entities[1].geometry).end,
            Point2::new(3.0, 4.0)
        );
        let polygon = shape::<Polyline2>(&back.entities[2].geometry);
        assert!(polygon.closed);
        assert_eq!(polygon.vertices.len(), 3);
        assert_eq!(shape::<Circle2>(&back.entities[3].geometry).radius, 8.0);
        let text = shape::<TextGeometry>(&back.entities[4].geometry);
        assert_eq!(text.text, "RL \"32.45\"");
        assert_eq!(text.height, 2.75);
        near(text.rotation, PI / 2.0, 1e-12);
    }

    #[test]
    fn an_arc_keeps_its_hand_and_which_of_the_two_arcs_it_is() {
        // Quarter turns about the origin from (10,0): counter-clockwise to
        // (0,10), clockwise to (0,-10); and the three-quarter turn the long
        // way to (0,10).
        for sweep in [PI / 2.0, -PI / 2.0, -1.5 * PI] {
            let mut model = Model::default();
            add_geometry(
                &mut model,
                Arc2::new(Point2::new(0.0, 0.0), 10.0, 0.0, sweep),
                "0",
            );
            let (back, exported) = round_trip(&model, &[]);

            let written = get::<VertexString>(&exported.archive.elements[0]);
            assert_eq!(written.segments.len(), 1);
            // The archive's positive radius turns right: counter-clockwise is
            // negative.
            assert_eq!(
                written.segments[0].radius,
                if sweep > 0.0 { -10.0 } else { 10.0 }
            );
            assert_eq!(written.segments[0].major, sweep.abs() > PI);

            // Back in Katana it is chords, every one of them on the original
            // circle and running the original way round.
            let chords = shape::<Polyline2>(&back.entities[0].geometry);
            assert!(chords.vertices.len() > 4);
            let mut turned = 0.0;
            for vertex in &chords.vertices {
                near(vertex.distance_to(Point2::new(0.0, 0.0)), 10.0, 1e-7);
            }
            for pair in chords.vertices.windows(2) {
                let (p, q) = (pair[0], pair[1]);
                turned += (p.x * q.y - p.y * q.x).atan2(p.x * q.x + p.y * q.y);
            }
            near(turned, sweep, 1e-6);
        }
    }

    #[test]
    fn heights_properties_and_twelve_ds_own_fields_survive_the_trip() {
        let mut model = Model::default();
        let mut string = make(
            polyline(&[(0.0, 0.0), (5.0, 0.0), (9.0, 3.0)], false).into(),
            "0",
        );
        string
            .properties
            .insert(String::from("elevations"), "1.5 null 2.25".into());
        string
            .properties
            .insert(String::from("Owner"), "Telstra".into());
        string
            .properties
            .insert(String::from("Ways"), PropertyValue::Integer(4));
        string
            .properties
            .insert(String::from("Asset/Dim/Cover"), PropertyValue::Real(0.9));
        string
            .properties
            .insert(String::from("Live"), PropertyValue::Boolean(true));
        string
            .metadata
            .insert(String::from("12d.name"), "Conduit 4-way".into());
        string.style = String::from("Comms-Conduit");
        string
            .metadata
            .insert(String::from("12d.colour"), "pen 025".into());
        string
            .metadata
            .insert(String::from("12d.chainage"), PropertyValue::Real(12.5));
        // A conduit is a LINE. Saying `point` here would be saying its three
        // vertices are three separate points, which is what that flag means
        // and what the import now does with it.
        string
            .metadata
            .insert(String::from("12d.breakline"), "line".into());
        string
            .metadata
            .insert(String::from("12d.point_ids"), "101,A 7,103".into());
        add(&mut model, string.clone());
        let mut point = make(
            PointGeometry {
                position: Point2::new(1.0, 1.0),
            }
            .into(),
            "0",
        );
        point
            .properties
            .insert(String::from("elevation"), PropertyValue::Real(31.25));
        add(&mut model, point);

        let (back, exported) = round_trip(&model, &[]);

        let written = get::<VertexString>(&exported.archive.elements[0]);
        assert_eq!(written.header.name, "Conduit 4-way");
        assert_eq!(written.vertices[1].z, None);
        assert_eq!(written.vertices[2].z, Some(2.25));
        assert_eq!(written.point_ids, vec!["101", "A 7", "103"]);
        // The group the path names is rebuilt, not written as a name with
        // slashes.
        let group = written
            .header
            .attributes
            .iter()
            .find(|attribute| attribute.name == "Asset")
            .expect("the Asset group");
        assert!(matches!(group.value, AttributeValue::Group(_)));

        assert_eq!(back.entities.len(), 2);
        let again = &back.entities[0];
        assert_eq!(prop_text(&again.properties, "elevations"), "1.5 null 2.25");
        assert_eq!(prop_text(&again.properties, "Owner"), "Telstra");
        assert_eq!(prop_integer(&again.properties, "Ways"), 4);
        assert_eq!(prop_real(&again.properties, "Asset/Dim/Cover"), 0.9);
        assert_eq!(
            prop_integer(&again.properties, "Live"),
            1,
            "the archive has no boolean attribute"
        );
        for key in [
            "12d.name",
            "12d.colour",
            "12d.chainage",
            "12d.breakline",
            "12d.point_ids",
        ] {
            assert_eq!(again.metadata.get(key), string.metadata.get(key), "{key}");
        }
        assert_eq!(
            again.style, "Comms-Conduit",
            "the style went out as the library linestyle and came back"
        );
        assert_eq!(written.header.style, "Comms-Conduit");
        assert_eq!(prop_real(&back.entities[1].properties, "elevation"), 31.25);
    }

    #[test]
    fn an_elevation_list_that_no_longer_fits_the_geometry_is_not_written_as_if_it_did() {
        let mut model = Model::default();
        let mut string = make(
            polyline(&[(0.0, 0.0), (5.0, 0.0), (9.0, 3.0)], false).into(),
            "0",
        );
        // A vertex was added since.
        string
            .properties
            .insert(String::from("elevations"), "1 2".into());
        add(&mut model, string);
        let out = export(&model, &[]);
        for vertex in &get::<VertexString>(&out.archive.elements[0]).vertices {
            assert!(vertex.z.is_none());
        }
    }

    #[test]
    fn a_resolved_custom_colour_keeps_its_name_until_the_user_changes_the_colour() {
        let archive = read_archive(
            "string super { colour \"test utility amber\" data_2d { 1 2 } }",
            &ReadOptions::default(),
        )
        .unwrap_or_else(|error| panic!("{}", error.describe()));
        let mut options = ImportOptions::default();
        options
            .colours
            .add("test utility amber", Color::new(224, 128, 32, 255))
            .expect("a colour of the customisation's own");
        let imported =
            to_domain(&archive, &options).unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(imported.entities.len(), 1);
        let mut model = Model::default();
        let mut unchanged = imported.entities[0].clone();
        unchanged.layer = String::from("0");
        add(&mut model, unchanged.clone());
        let mut edited = unchanged.clone();
        edited.color = Some(Color::new(0, 0, 255, 255));
        add(&mut model, edited);
        let mut inherited = unchanged;
        inherited.color = None;
        add(&mut model, inherited);
        let exported = export(&model, &[]);
        assert_eq!(exported.archive.elements.len(), 3);
        let colour = |index: usize| {
            &get::<VertexString>(&exported.archive.elements[index])
                .header
                .colour
        };
        assert_eq!(colour(0), "test utility amber");
        assert_eq!(colour(1), "blue");
        assert_eq!(colour(2), "white");
    }

    #[test]
    fn the_colour_written_is_the_name_it_came_with_unless_the_colour_has_changed() {
        let mut model = Model::default();
        let mut kept = make(
            PointGeometry {
                position: Point2::new(0.0, 0.0),
            }
            .into(),
            "0",
        );
        kept.color = Some(Color::new(255, 255, 0, 255));
        kept.metadata
            .insert(String::from("12d.colour"), "Yellow".into());
        add(&mut model, kept);
        let mut recoloured = make(
            PointGeometry {
                position: Point2::new(1.0, 0.0),
            }
            .into(),
            "0",
        );
        recoloured.color = Some(Color::new(0, 0, 250, 255));
        recoloured
            .metadata
            .insert(String::from("12d.colour"), "yellow".into());
        add(&mut model, recoloured);
        // ByLayer; layer "0" is white
        add_geometry(
            &mut model,
            PointGeometry {
                position: Point2::new(2.0, 0.0),
            },
            "0",
        );

        let out = export(&model, &[]);
        let colour_of = |index: usize| {
            &get::<VertexString>(&out.archive.elements[index])
                .header
                .colour
        };
        assert_eq!(
            colour_of(0),
            "Yellow",
            "its own spelling, since it still means the same colour"
        );
        assert_eq!(colour_of(1), "blue", "the old name would be a lie");
        assert_eq!(colour_of(2), "white");
    }

    #[test]
    fn dimensions_are_counted_as_skipped_and_only_chosen_entities_are_written() {
        let mut model = Model::default();
        add_geometry(
            &mut model,
            DimensionGeometry::new(Point2::new(0.0, 0.0), Point2::new(10.0, 0.0), 2.0, ""),
            "0",
        );
        let wanted = add_geometry(
            &mut model,
            PointGeometry {
                position: Point2::new(1.0, 1.0),
            },
            "0",
        );
        assert_ne!(wanted, INVALID_ENTITY_ID);
        add_geometry(
            &mut model,
            PointGeometry {
                position: Point2::new(2.0, 2.0),
            },
            "0",
        );

        let all = export(&model, &[]);
        assert_eq!(all.entities_written, 2);
        assert_eq!(all.entities_skipped, 1);
        assert_eq!(all.warnings.len(), 1);
        assert!(
            all.warnings[0].contains("1 dimensions"),
            "{}",
            all.warnings[0]
        );

        let options = ExportOptions {
            entities: vec![wanted],
            ..ExportOptions::default()
        };
        let some = from_domain(&model, &[], &options, &[])
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(some.entities_written, 1);
        assert_eq!(some.archive.elements.len(), 1);
    }

    #[test]
    fn an_origin_shift_is_added_back_so_coordinates_return_to_where_they_came_from() {
        let mut model = Model::default();
        add_geometry(
            &mut model,
            PointGeometry {
                position: Point2::new(10.0, 20.0),
            },
            "0",
        );
        let options = ExportOptions {
            origin_shift: Some(Vec2::new(502000.0, 6960000.0)),
            ..ExportOptions::default()
        };
        let out = from_domain(&model, &[], &options, &[])
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(
            get::<VertexString>(&out.archive.elements[0]).vertices[0],
            support::at(502010.0, 6960020.0, None)
        );
    }

    #[test]
    fn an_alignment_with_spirals_is_written_solved_and_reads_back_as_the_same_pis() {
        // This closes the loop on the trailing-transition convention: Katana
        // writes its outgoing spiral described backwards, as the format's
        // program does, and the importer - which checks every recorded vertex
        // against what it solves - accepts it. With either half of the
        // convention wrong the check fails.
        let mut model = Model::default();
        let pi = |x: f64, y: f64, radius: f64, spiral_in: f64, spiral_out: f64| AlignmentPi {
            point: Point2::new(x, y),
            radius,
            spiral_in,
            spiral_out,
        };
        let mut alignment = Alignment {
            name: String::from("MC01"),
            ..Alignment::default()
        };
        alignment.horizontal.start_station = 1000.0;
        alignment.horizontal.pis = vec![
            pi(0.0, 0.0, 0.0, 0.0, 0.0),
            pi(500.0, 0.0, 200.0, 60.0, 40.0),
            pi(700.0, 500.0, 150.0, 0.0, 0.0),
            pi(1300.0, 500.0, 0.0, 0.0, 0.0),
        ];
        let pvi = |station: f64, elevation: f64, curve_length: f64| ProfilePvi {
            station,
            elevation,
            curve_length,
        };
        alignment.vertical = Some(VerticalAlignment {
            pvis: vec![
                pvi(1000.0, 30.0, 0.0),
                pvi(1400.0, 42.0, 120.0),
                pvi(2000.0, 36.0, 0.0),
            ],
        });
        assert!(model.alignments.add(alignment.clone()).is_ok());

        let mut out = export(&model, &[]);
        assert_eq!(out.alignments_written, 1);
        let Element::SuperAlignment(written) = &mut out.archive.elements[0] else {
            panic!("the alignment is written as a super alignment");
        };
        assert!(written.horizontal_is_ip_only());
        let horizontal = written.horizontal_data.as_ref().expect("solved geometry");
        // tangent, spiral, arc, spiral, tangent, arc, tangent
        assert_eq!(horizontal.segments.len(), 7);
        assert_eq!(
            horizontal.segments[1].parameters.integer("leading"),
            Some(1)
        );
        assert_eq!(
            horizontal.segments[3].parameters.integer("leading"),
            Some(0)
        );
        assert_eq!(
            horizontal.segments[3].parameters.real("r1"),
            Some(0.0),
            "a trailing transition is described from its straight end"
        );

        // Force the importer down the harder road: no IPs, only solved
        // elements.
        written.horizontal_parts.clear();
        written.vertical_parts.clear();
        let archive = read_archive(
            write_archive(&out.archive, &WriteOptions::default()),
            &ReadOptions::default(),
        )
        .unwrap_or_else(|error| panic!("{}", error.describe()));
        let back = to_domain(&archive, &ImportOptions::default())
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(back.alignments.len(), 1, "{:?}", back.warnings.first());
        let pis = &back.alignments[0].horizontal.pis;
        assert_eq!(pis.len(), 4);
        for (i, (got, want)) in pis.iter().zip(&alignment.horizontal.pis).enumerate() {
            // Coordinates cross the file at 8 decimal places.
            assert!((got.point.x - want.point.x).abs() <= 1e-5, "{i}");
            assert!((got.point.y - want.point.y).abs() <= 1e-5, "{i}");
            assert!((got.radius - want.radius).abs() <= 1e-6, "{i}");
            assert!((got.spiral_in - want.spiral_in).abs() <= 1e-6, "{i}");
            assert!((got.spiral_out - want.spiral_out).abs() <= 1e-6, "{i}");
        }
        assert_eq!(back.alignments[0].horizontal.start_station, 1000.0);
        let vertical = back.alignments[0].vertical.as_ref().expect("a grade line");
        assert_eq!(vertical.pvis.len(), 3);
        near(vertical.pvis[1].station, 1400.0, 1e-6);
        near(vertical.pvis[1].elevation, 42.0, 1e-6);
        near(vertical.pvis[1].curve_length, 120.0, 1e-6);
    }

    #[test]
    fn a_surface_is_written_clockwise_and_reads_back_as_the_same_ground() {
        // Two triangles over a 10 m square tilted in x: z = 5 + 0.1 x.
        let surface = TinSurface::create(
            vec![
                Point3::new(0.0, 0.0, 5.0),
                Point3::new(10.0, 0.0, 6.0),
                Point3::new(10.0, 10.0, 6.0),
                Point3::new(0.0, 10.0, 5.0),
            ],
            vec![[0, 1, 2], [0, 2, 3]],
            Vec::new(),
        )
        .unwrap_or_else(|error| panic!("{}", error.describe()));
        let model = Model::default();

        let (back, exported) = round_trip(
            &model,
            &[ExportSurface {
                name: String::from("DESIGN"),
                surface: Some(&surface),
            }],
        );
        assert_eq!(exported.surfaces_written, 1);
        let tin = get::<Tin>(&exported.archive.elements[0]);
        assert!(!tin.full);
        // (0,0) (10,10) (10,0): clockwise seen from above, as the manual
        // requires.
        assert_eq!(tin.triangles[0], [0, 2, 1]);

        assert_eq!(back.surfaces.len(), 1);
        assert_eq!(back.surfaces[0].name, "DESIGN");
        assert_eq!(back.surfaces[0].surface.triangle_count(), 2);
        let z = back.surfaces[0]
            .surface
            .elevation_at_xy(2.5, 7.5)
            .expect("on the surface");
        near(z, 5.25, 1e-12);
        near(back.surfaces[0].surface.plan_area(), 100.0, 1e-9);
    }

    #[test]
    fn a_surfaces_coordinates_cross_the_file_without_losing_a_bit() {
        // Thirds and sevenths at survey magnitude, which eight decimal places
        // cannot hold. Eight places moves a point by up to 5e-9 - twenty times
        // less than the tolerance at which a triangle counts as flat, so it
        // cannot turn a real triangle inside out, but it is still not ours to
        // throw away: a tin's coordinates are computed, not typed.
        let x = 187008.0 + 1.0 / 3.0;
        let y = 6184863.0 + 1.0 / 7.0;
        let surface = TinSurface::create(
            vec![
                Point3::new(x, y, 8.0 + 2.0 / 3.0),
                Point3::new(x + 10.0, y, 9.5),
                Point3::new(x + 10.0, y + 10.0, 10.25),
                Point3::new(x, y + 10.0, 11.0),
            ],
            vec![[0, 1, 2], [0, 2, 3]],
            Vec::new(),
        )
        .unwrap_or_else(|error| panic!("{}", error.describe()));
        let model = Model::default();
        let (back, _) = round_trip(
            &model,
            &[ExportSurface {
                name: String::from("DESIGN"),
                surface: Some(&surface),
            }],
        );
        assert_eq!(back.surfaces.len(), 1);

        // The importer numbers vertices in the order the triangles first use
        // them, so compare the sets rather than the lists.
        let sorted = |points: &[Point3]| {
            let mut points = points.to_vec();
            points.sort_by(|a, b| {
                let order = |p: f64, q: f64| p.partial_cmp(&q).unwrap_or(Ordering::Equal);
                order(a.x, b.x).then(order(a.y, b.y)).then(order(a.z, b.z))
            });
            points
        };
        assert_eq!(
            sorted(back.surfaces[0].surface.vertices().as_slice()),
            sorted(surface.vertices().as_slice()),
            "every vertex must arrive as exactly the double it left as"
        );
    }

    #[test]
    fn an_empty_model_is_an_empty_archive_and_bad_options_are_refused() {
        let model = Model::default();
        let out = export(&model, &[]);
        assert!(out.archive.elements.is_empty());
        let options = ExportOptions {
            curve_tolerance: -1.0,
            ..ExportOptions::default()
        };
        assert!(from_domain(&model, &[], &options, &[]).is_err());
    }

    #[test]
    fn renaming_a_style_in_katana_does_not_rename_the12d_linestyle_or_symbol_it_writes() {
        // Three strings, one of each way a library name reaches a style: main,
        // a line in linestyle "WATR Main"; mh, a point on linestyle "0" drawn
        // with symbol "SEWR Manhole Cover", so its style is the symbol's and
        // "0" is kept aside; tree, a point whose linestyle and symbol are both
        // "TREE Canopy", so nothing is kept aside and the style is all there
        // is.
        let model = imported_with_styles_renamed(
            r#"breakline line
string super { name "main" colour blue style "WATR Main" data_3d { 0 0 0  5 0 0 } }
string super { name "mh" breakline point colour yellow style "0" data_3d { 100 200 5.5 }
  symbol_value { style "SEWR Manhole Cover" colour yellow size 1 rotation 0 offset 0 raise 0 } }
string super { name "tree" breakline point colour green style "TREE Canopy" data_3d { 7 7 0 }
  symbol_value { style "TREE Canopy" colour green size 2 rotation 0 offset 0 raise 0 } }"#,
        );
        assert!(
            model.styles.find("Katana WATR Main").is_some(),
            "the rename took"
        );
        assert!(model.styles.find("WATR Main").is_none());

        let out = export(&model, &[]);
        assert_eq!(out.archive.elements.len(), 3);
        let main = get::<VertexString>(&out.archive.elements[0]);
        let manhole = get::<VertexString>(&out.archive.elements[1]);
        let tree = get::<VertexString>(&out.archive.elements[2]);

        assert_eq!(main.header.style, "WATR Main");
        assert!(main.symbol.is_none());
        assert_eq!(manhole.header.style, "0");
        let symbol = manhole.symbol.as_ref().expect("a symbol");
        assert_eq!(symbol.text("style", ""), "SEWR Manhole Cover");
        assert_eq!(tree.header.style, "TREE Canopy");
        let symbol = tree.symbol.as_ref().expect("a symbol");
        assert_eq!(symbol.text("style", ""), "TREE Canopy");
    }

    #[test]
    fn a_katana_style_writes_its_linetype_and_continuous_is12ds_default_linestyle() {
        let mut model = Model::default();
        let kerbs = Layer {
            name: String::from("KERBS"),
            linetype: String::from("KERB Barrier"),
            ..Layer::default()
        };
        assert!(model.layers.add(kerbs).is_ok());
        for (name, linetype) in [
            ("Fence", "FENC Post and Wire"),
            ("Plain", "Continuous"), // any case: Katana's own solid line
            ("Inherit", "ByLayer"),
        ] {
            let made = Style {
                name: String::from(name),
                linetype: String::from(linetype),
                ..Style::default()
            };
            assert!(model.styles.add(made).is_ok());
        }
        let line = polyline(&[(0.0, 0.0), (1.0, 0.0)], false);
        for (style, layer) in [
            ("Fence", "0"),
            ("Plain", "0"),
            ("Inherit", "KERBS"),
            ("", "KERBS"),         // ByLayer
            ("", "0"),             // ByLayer, on a continuous layer
            ("Not In Model", "0"), // the name is all there is
        ] {
            let mut entity = make(line.clone().into(), layer);
            entity.style = String::from(style);
            add(&mut model, entity);
        }

        let out = export(&model, &[]);
        assert_eq!(out.archive.elements.len(), 6);
        let style_of = |i: usize| &get::<VertexString>(&out.archive.elements[i]).header.style;
        assert_eq!(style_of(0), "FENC Post and Wire");
        assert_eq!(
            style_of(1),
            "1",
            "the format has no \"continuous\"; its solid default is \"1\""
        );
        assert_eq!(
            style_of(2),
            "KERB Barrier",
            "ByLayer on the style is the layer's linetype"
        );
        assert_eq!(style_of(3), "KERB Barrier");
        assert_eq!(style_of(4), "1");
        assert_eq!(style_of(5), "Not In Model");
    }
}
