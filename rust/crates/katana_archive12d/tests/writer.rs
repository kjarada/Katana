//! The twin of tests/archive12d/test_writer.cpp: what is read is written
//! back, and the text the writer chooses for it.

#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

pub mod support;

use katana_archive12d::{
    AlignmentPart, AlignmentSource, ArcString, Archive, Attribute, AttributeValue, Breakline,
    CircleString, DrainageRecord, DrainageString, FieldList, LasCloud, LasPoint, ModelRecord,
    PlotFrame, ReadOptions, Segment, SegmentKind, SolvedGeometry, StringHeader, StringKind,
    SuperAlignment, SuperTin, TextString, Tin, Trimesh, TrimeshInfo, VertexString, WriteOptions,
    element_keyword, quoted, read_archive, read_archive_bytes, write_archive,
};

use support::{at, attribute, fixture, get, text_attribute};

fn reread(archive: &Archive, options: &WriteOptions) -> Archive {
    let text = write_archive(archive, options);
    match read_archive(text.as_bytes(), &ReadOptions::default()) {
        Ok(back) => back,
        Err(error) => panic!("{}\n{text}", error.describe()),
    }
}

fn header(name: &str, model: &str) -> StringHeader {
    StringHeader {
        name: String::from(name),
        model: String::from(model),
        colour: String::from("dark green"),
        style: String::from("Kerb & Channel"),
        chainage: 12.5,
        breakline: Breakline::Line,
        ..StringHeader::default()
    }
}

// What the writer is documented to change: the superseded kinds are written
// as super strings, and a full_tin without neighbours as a tin.
fn expect_same_elements(written: &Archive, back: &Archive) {
    assert_eq!(written.elements.len(), back.elements.len());
    for (i, (before, after)) in written.elements.iter().zip(&back.elements).enumerate() {
        assert!(
            before == after,
            "element {i} ({}) did not survive being written and read",
            element_keyword(before)
        );
    }
}

mod quoting {
    use super::*;

    #[test]
    fn a_double_quote_and_a_backslash_are_escaped_and_nothing_else() {
        // Manual 1.1.
        assert_eq!(quoted("plain"), "\"plain\"");
        assert_eq!(quoted(""), "\"\"");
        assert_eq!(quoted("say \"hi\""), r#""say \"hi\"""#);
        assert_eq!(quoted(r"$LIB\title.tbf"), r#""$LIB\\title.tbf""#);
        assert_eq!(quoted("a // b"), "\"a // b\"");
    }
}

mod writer {
    use super::*;

    #[test]
    fn every_documented_element_reads_back_as_it_was_written() {
        let mut archive = Archive::default();
        archive
            .project_attributes
            .push(text_attribute("Zone", "MGA 56"));
        archive.models.push(ModelRecord {
            name: String::from("Survey/Control"),
            attributes: vec![text_attribute("Owner", "QLD")],
            extras: FieldList::new(),
        });

        // A super string using every optional block.
        let mut super_string = VertexString {
            header: header("Kerb \"north\"", "Survey/Control"),
            ..VertexString::default()
        };
        super_string
            .header
            .attributes
            .push(attribute("Height", AttributeValue::Real(5.25)));
        super_string
            .header
            .attributes
            .push(attribute("Wires", AttributeValue::Integer(3)));
        super_string.header.attributes.push(Attribute {
            name: String::from("this id"),
            value: AttributeValue::Integer(5292),
            declared_type: String::from("uid"),
        });
        super_string.header.attributes.push(attribute(
            "Asset",
            AttributeValue::Group(vec![
                text_attribute("Type", "Kerb"),
                attribute("Size", AttributeValue::Real(0.125)),
            ]),
        ));
        super_string.header.extras.add("weight", "2", false);
        super_string
            .header
            .extras
            .add("time_created", "07-Mar-2009 03:53:17", true);
        super_string.closed = true;
        super_string.vertices = vec![
            at(0.0, 0.0, Some(1.5)),
            at(10.0, 0.0, None),
            at(10.0, 10.0, Some(2.25)),
        ];
        let arc = Segment {
            kind: SegmentKind::Arc,
            radius: -7.5,
            major: true,
            ..Segment::default()
        };
        super_string.segments = vec![Segment::default(), arc.clone(), Segment::default()];
        super_string.point_ids = vec![
            String::from("101"),
            String::from("A 7"),
            String::from("103"),
        ];
        super_string.vertex_text = vec![String::from("a"), String::new(), String::from("c")];
        super_string.segment_colours = vec![
            String::from("red"),
            String::from("dark blue"),
            String::from("-1"),
        ];
        super_string.diameters = vec![0.25, 0.5, 0.75];
        super_string.justify = String::from("invert");
        super_string.vertex_tinable = vec![true, false, true];
        super_string.segment_visible_value = Some(false);
        let mut annotation = FieldList::new();
        annotation.add("worldsize", "2.5", false);
        annotation.add("justify", "top-left", true);
        super_string.vertex_annotation = Some(annotation);
        super_string.symbols = vec![FieldList::new(); 3];
        super_string.symbols[0].add("style", "Tree", true);
        super_string.vertex_attributes = vec![
            vec![text_attribute("n", "1")],
            vec![],
            vec![text_attribute("n", "3")],
        ];
        let mut interval = FieldList::new();
        interval.add("chord_arc", "0.01", false);
        super_string.interval = Some(interval);
        archive.elements.push(super_string.into());

        // Transitions need geometry_data.
        let mut transition = VertexString {
            header: header("Transition", "Design"),
            constant_z: Some(31.25),
            vertices: vec![at(0.0, 0.0, None), at(80.0, 4.0, None)],
            ..VertexString::default()
        };
        let mut spiral = Segment {
            kind: SegmentKind::Spiral,
            ..Segment::default()
        };
        spiral.parameters.add("type", "cubic parabola", true);
        spiral.parameters.add("leading", "1", false);
        spiral.parameters.add("l2", "80", false);
        spiral.parameters.add("r2", "-210.5", false);
        transition.segments = vec![spiral];
        archive.elements.push(transition.into());

        let interface = VertexString {
            kind: StringKind::Interface,
            header: header("Daylight", "Design"),
            vertices: vec![at(0.0, 0.0, Some(1.0)), at(5.0, 0.0, Some(2.0))],
            interface_modes: vec![-1, 1],
            ..VertexString::default()
        };
        archive.elements.push(interface.into());

        let mut face = VertexString {
            kind: StringKind::Face,
            header: header("Hatch", "Design"),
            vertices: vec![
                at(0.0, 0.0, Some(1.0)),
                at(5.0, 0.0, Some(1.0)),
                at(5.0, 5.0, Some(1.0)),
            ],
            ..VertexString::default()
        };
        face.header.extras.add("hatch_angle", "45", false);
        archive.elements.push(face.into());

        let arc_string = ArcString {
            header: header("Arc", "Design"),
            radius: -15.0,
            centre: at(600.0, 100.0, Some(31.0)),
            start: at(615.0, 100.0, Some(31.0)),
            end: at(600.0, 115.0, None),
        };
        archive.elements.push(arc_string.into());

        let mut circle = CircleString {
            header: header("Tank", "Design"),
            radius: 8.0,
            centre: at(650.0, 200.0, Some(32.0)),
            ..CircleString::default()
        };
        archive.elements.push(circle.clone().into());
        circle.feature = true;
        circle.header.name = String::from("Tree");
        archive.elements.push(circle.into());

        let mut label = TextString {
            header: header("", "Labels"),
            text: String::from("RL \"32.451\""),
            position: at(1.0, 2.0, None),
            ..TextString::default()
        };
        label.annotation.add("worldsize", "2.5", false);
        label.annotation.add("textstyle", "Arial Narrow", true);
        archive.elements.push(label.into());

        let mut frame = PlotFrame {
            header: header("Sheet 01", "Plot Frames"),
            ..PlotFrame::default()
        };
        frame.fields.add("width", "420", false);
        frame.fields.add("title_file", r"$LIB\title.tbf", true);
        archive.elements.push(frame.into());

        let mut pit = DrainageRecord::default();
        pit.fields.add("name", "A1", true);
        pit.fields.add("diameter", "1.05", false);
        pit.attributes.push(text_attribute("Lid", "Class D"));
        let mut pipe = DrainageRecord::default();
        pipe.fields.add("us_level", "29.5", false);
        let mut control = DrainageRecord::default();
        control.fields.add("name", "Lot 4", true);
        control.vertices = vec![at(25.0, 10.0, Some(29.25)), at(30.0, 40.0, Some(30.0))];
        let drainage = DrainageString {
            header: header("SW Line A", "Drainage"),
            outfall: Some(28.5),
            flow_direction: Some(1),
            vertices: vec![at(0.0, 0.0, Some(29.0)), at(50.0, 20.0, Some(29.5))],
            pits: vec![pit.clone(), pit],
            pits_are_version2: true,
            pipes: vec![pipe.clone()],
            property_controls: vec![control],
            house_connections: vec![pipe],
            ..DrainageString::default()
        };
        archive.elements.push(drainage.into());

        let mut ip = AlignmentPart {
            kind: String::from("spiral"),
            ..AlignmentPart::default()
        };
        ip.fields.add("id", "300", false);
        ip.fields.add("r", "50", false);
        ip.attributes.push(text_attribute("note", "x"));
        let mut horizontal = SolvedGeometry::default();
        horizontal.header.add("name", "MC01", true);
        horizontal.vertices = vec![
            at(0.0, 0.0, None),
            at(100.0, 0.0, None),
            at(150.0, 50.0, None),
        ];
        horizontal.segments = vec![Segment::default(), arc];
        let mut vertical = SolvedGeometry {
            vertices: vec![at(0.0, 30.0, None), at(100.0, 33.0, None)],
            ..SolvedGeometry::default()
        };
        let mut parabola = Segment {
            kind: SegmentKind::Parabola,
            ..Segment::default()
        };
        parabola.parameters.add("chainage", "50", false);
        parabola.parameters.add("height", "35", false);
        vertical.segments = vec![parabola];
        let alignment = SuperAlignment {
            header: header("MC01", "Alignments"),
            spiral_type: String::from("clothoid"),
            valid_horizontal: Some(true),
            valid_vertical: Some(false),
            horizontal_parts: vec![ip],
            horizontal_data: Some(horizontal),
            vertical_data: Some(vertical),
            diameter: Some(0.375),
            pipe_length: Some(6.0),
            ..SuperAlignment::default()
        };
        archive.elements.push(alignment.into());

        let mut cloud = LasCloud {
            header: header("Scan", "Clouds"),
            format: String::from("v14_p8"),
            point_format: 8,
            categories: vec![true, false],
            ..LasCloud::default()
        };
        cloud.range.add("xmin", "1", false);
        let point = LasPoint {
            x: 10.5,
            y: 20.5,
            z: 3.25,
            intensity: 1200,
            return_number: 5,
            return_count: 6,
            classification_flags: 7,
            scanner_channel: 2,
            scan_direction: 1,
            classification: 4,
            user_data: 200,
            scan_angle: -30,
            point_source_id: 9,
            gps_time: 55.5,
            colour: 281474976710655,
            near_infrared: 17,
            ..LasPoint::default()
        };
        cloud.points = vec![point, point];
        archive.elements.push(cloud.into());
        let reference = LasCloud {
            header: header("Ref", "Clouds"),
            reference_file: String::from("scans/site.las"),
            ..LasCloud::default()
        };
        archive.elements.push(reference.into());

        let mut tin = Tin {
            full: true,
            name: String::from("NATURAL SURFACE"),
            colour: String::from("green"),
            attributes: vec![text_attribute("Style", "1")],
            points: vec![
                at(-100.0, -100.0, Some(0.0)),
                at(-100.0, 100.0, Some(0.0)),
                at(100.0, 100.0, Some(0.0)),
                at(100.0, -100.0, Some(0.0)),
                at(0.0, 0.0, Some(5.0)),
                at(10.0, 0.0, None),
                at(0.0, 10.0, Some(7.5)),
            ],
            triangles: vec![[4, 6, 5], [0, 4, 5]],
            neighbours: vec![
                [Tin::NO_NEIGHBOUR, Tin::NO_NEIGHBOUR, 1],
                [Tin::NO_NEIGHBOUR, 0, Tin::NO_NEIGHBOUR],
            ],
            visible: vec![true, false],
            colours: vec![String::from("-1"), String::from("vis grass")],
            input_models: vec![String::from("SURVEY DETAILS")],
            ..Tin::default()
        };
        tin.input.add("weed_tin", "false", false);
        archive.elements.push(tin.into());

        let super_tin = SuperTin {
            name: String::from("COMBINED"),
            colour: String::from("green"),
            tins: vec![String::from("NATURAL SURFACE"), String::from("PAD A")],
            ..SuperTin::default()
        };
        archive.elements.push(super_tin.into());

        let mut mesh = Trimesh {
            name: String::from("Stockpile SP-02"),
            model: String::from("Meshes"),
            colour: String::from("brown"),
            vertices: vec![
                at(0.0, 0.0, Some(0.0)),
                at(20.0, 0.0, Some(0.0)),
                at(20.0, 20.0, Some(0.0)),
                at(10.0, 10.0, Some(3.5)),
            ],
            faces: vec![[0, 1, 3], [1, 2, 3]],
            edges: vec![[0, 1]],
            blend: Some(0.5),
            face_infos: vec![TrimeshInfo {
                flag: 0,
                key: 1,
                colour: String::from("blue"),
                name: String::from("side face"),
            }],
            face_flags: vec![0, 0],
            ..Trimesh::default()
        };
        mesh.info.add("key", "255", false);
        archive.elements.push(mesh.into());

        let back = reread(&archive, &WriteOptions::default());
        expect_same_elements(&archive, &back);
        assert_eq!(back.project_attributes, archive.project_attributes);
        assert_eq!(back.models, archive.models);
        assert!(back.unrecognised.is_empty());
        assert!(back.warnings.is_empty(), "{:?}", back.warnings.first());
    }

    #[test]
    fn superseded_strings_are_written_as_the_super_strings_that_replaced_them() {
        let read = read_archive(
            r#"model "Legacy"
string 2d { name "two" z 12.5 data { 0 0  5 5 } }
string 4d { name "four" angle 15 worldsize 2 data { 0 0 1 "T1"  5 5 2 "T2" } }
string pipe { name "pipe" diameter 0.5 data { 0 0 1  5 5 2 } }
string polyline { name "poly" closed 1 data { 0 0 1 12 1   20 0 1 0 0   20 20 1 0 0 } }"#,
            &ReadOptions::default(),
        )
        .unwrap_or_else(|error| panic!("{}", error.describe()));
        let text = write_archive(&read, &WriteOptions::default());
        for old in ["string 2d", "string 4d", "string pipe", "string polyline"] {
            assert!(
                !text.contains(old),
                "{old} is superseded and is not written"
            );
        }
        let back = read_archive(text.as_bytes(), &ReadOptions::default())
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(back.elements.len(), 4);
        for (i, (before, after)) in read.elements.iter().zip(&back.elements).enumerate() {
            let mut expected = get::<VertexString>(before).clone();
            expected.kind = StringKind::Super; // the one documented difference
            assert!(expected == *get::<VertexString>(after), "string {i}");
        }
    }

    #[test]
    fn an_old_alignment_is_written_as_a_super_alignment_with_the_same_ips() {
        let read = read_archive(
            "string alignment { name \"AL\" hipdata { 0 0 0  100 0 40  150 100 0 }\n\
             \x20vipdata { 0 30 0  100 33 40 parabola  220 31 0 } }",
            &ReadOptions::default(),
        )
        .unwrap_or_else(|error| panic!("{}", error.describe()));
        let back = read_archive(
            write_archive(&read, &WriteOptions::default()),
            &ReadOptions::default(),
        )
        .unwrap_or_else(|error| panic!("{}", error.describe()));
        let mut expected = get::<SuperAlignment>(&read.elements[0]).clone();
        expected.source = AlignmentSource::SuperAlignment;
        assert!(expected == *get::<SuperAlignment>(&back.elements[0]));
    }

    #[test]
    fn a_tin_without_neighbours_is_written_in_the_form_that_does_not_need_them() {
        // Manual 1.4.7.1 makes neighbours MANDATORY in a full_tin; 1.4.7.2's
        // tin lists visible triangles only.
        let mut archive = Archive::default();
        let tin = Tin {
            full: true,
            name: String::from("T"),
            points: vec![
                at(0.0, 0.0, Some(0.0)),
                at(0.0, 9.0, Some(0.0)),
                at(9.0, 9.0, Some(0.0)),
                at(9.0, 0.0, Some(0.0)),
                at(1.0, 1.0, Some(5.0)),
                at(2.0, 1.0, Some(5.0)),
                at(1.0, 2.0, Some(5.0)),
                at(2.0, 2.0, Some(5.0)),
            ],
            triangles: vec![[4, 6, 5], [5, 6, 7], [0, 4, 5]],
            visible: vec![true, false, true],
            ..Tin::default()
        };
        archive.elements.push(tin.into());

        let text = write_archive(&archive, &WriteOptions::default());
        assert!(!text.contains("full_tin"));
        assert!(!text.contains("neighbours"));
        let back = read_archive(text.as_bytes(), &ReadOptions::default())
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        let written = get::<Tin>(&back.elements[0]);
        assert!(!written.full);
        // Of the three, one is nulled and one touches a construction point.
        assert_eq!(written.triangles.len(), 1);
        assert_eq!(written.triangles[0], [4, 6, 5]);
    }

    #[test]
    fn a_null_height_is_written_as_the_archives_null_value() {
        let mut archive = Archive {
            null_value: -1.0,
            ..Archive::default()
        };
        let string = VertexString {
            header: header("S", "M"),
            vertices: vec![at(0.0, 0.0, Some(5.0)), at(1.0, 1.0, None)],
            ..VertexString::default()
        };
        archive.elements.push(string.into());
        let text = write_archive(&archive, &WriteOptions::default());
        assert!(text.contains("null -1\n"), "{text}");
        assert!(text.contains("1 1 -1\n"), "{text}");
        let back = read_archive(text.as_bytes(), &ReadOptions::default())
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(
            get::<VertexString>(&back.elements[0]).vertices[1]
                .z
                .is_none()
        );
    }

    #[test]
    fn tin_points_read_back_bit_for_bit_because_hexadecimal_is_the_default() {
        let mut archive = Archive::default();
        // Thirds and sevenths: no finite decimal, so eight places cannot hold
        // them.
        let tin = Tin {
            name: String::from("T"),
            points: vec![
                at(
                    187008.0 + 1.0 / 3.0,
                    6184863.0 + 1.0 / 7.0,
                    Some(8.0 + 2.0 / 3.0),
                ),
                at(187009.5, 6184864.25, Some(-0.1)),
                at(187010.0, 6184865.0, Some(1e-9)),
            ],
            triangles: vec![[0, 2, 1]],
            ..Tin::default()
        };
        let points = tin.points.clone();
        archive.elements.push(tin.into());

        // No options at all: a tin's coordinates are computed rather than
        // typed, and the orientation of a sliver triangle lives in their last
        // bits, so the default follows the writing program's
        // (`output_tin_hex_floats true`).
        let back = reread(&archive, &WriteOptions::default());
        assert_eq!(get::<Tin>(&back.elements[0]).points, points);

        // ... which decimal at eight places does not quite manage.
        let options = WriteOptions {
            hex_float_tins: false,
            ..WriteOptions::default()
        };
        let decimal = reread(&archive, &options);
        assert_ne!(get::<Tin>(&decimal.elements[0]).points, points);
    }

    #[test]
    fn coordinates_are_written_without_exponents_or_trailing_zeros() {
        let mut archive = Archive::default();
        let string = VertexString {
            header: header("S", "M"),
            vertices: vec![
                at(502000.0, 6960000.5, Some(0.000125)),
                at(-0.00000001, 1e7, Some(-30.25)),
            ],
            ..VertexString::default()
        };
        archive.elements.push(string.into());
        let text = write_archive(&archive, &WriteOptions::default());
        assert!(text.contains("502000 6960000.5 0.000125\n"), "{text}");
        // -0.00000001 is one unit in the eighth place; 1e7 must not become
        // 1e+07.
        assert!(text.contains("-0.00000001 10000000 -30.25\n"), "{text}");

        let coarse = WriteOptions {
            decimal_places: 3,
            ..WriteOptions::default()
        };
        let rounded = write_archive(&archive, &coarse);
        assert!(rounded.contains("502000 6960000.5 0\n"), "{rounded}");
        assert!(
            rounded.contains("\n    0 10000000 -30.25\n"),
            "a value that rounds to zero is 0, never -0\n{rounded}"
        );
    }

    #[test]
    fn a_model_is_named_once_for_a_run_of_its_elements() {
        let mut archive = Archive::default();
        for model in ["A", "A", "B", "A"] {
            let string = VertexString {
                header: header("S", model),
                vertices: vec![at(0.0, 0.0, Some(0.0))],
                ..VertexString::default()
            };
            archive.elements.push(string.into());
        }
        let text = write_archive(&archive, &WriteOptions::default());
        let commands = text.matches("\nmodel ").count();
        assert_eq!(commands, 3, "{text}"); // A, B, A
        let back = read_archive(text.as_bytes(), &ReadOptions::default())
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(get::<VertexString>(&back.elements[3]).header.model, "A");
    }

    #[test]
    fn the_all_geometries_fixture_survives_being_written_and_read_again() {
        let first = read_archive_bytes(fixture("all_geometries.12da"), &ReadOptions::default())
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        let second = read_archive(
            write_archive(&first, &WriteOptions::default()),
            &ReadOptions::default(),
        )
        .unwrap_or_else(|error| panic!("{}", error.describe()));
        // A second trip must change nothing at all: whatever the writer
        // normalises it normalises once.
        let third = read_archive(
            write_archive(&second, &WriteOptions::default()),
            &ReadOptions::default(),
        )
        .unwrap_or_else(|error| panic!("{}", error.describe()));
        expect_same_elements(&second, &third);
        assert_eq!(second.elements.len(), first.elements.len());
        // Including "Ground TIN", which holds no string at all: a model that
        // is empty is still a model.
        let mut before = first.model_names.clone();
        let mut after = second.model_names.clone();
        before.sort();
        after.sort();
        assert_eq!(after, before);
    }
}
