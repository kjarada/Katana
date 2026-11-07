//! Twin of tests/ifc/test_export.cpp: the IFC 4.3 file the export writes, read
//! back instance by instance. What each test expects is worked out beside it -
//! from the schema, from AS 5488, from IfcOpenShell's encodings - never read
//! back from what the writer happened to write.

pub mod support;

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use katana_core::ErrorCode;
use katana_entity::annotation::{LabelKind, LabelStyle};
use katana_entity::entity::{
    AnchorPoint, AnchorRef, ELEVATION_PROPERTY, Entity, EntityId, EntityType, Geometry,
    LeaderGeometry, PointGeometry, PropertyValue, TextGeometry, heights_of, property_to_string,
    set_heights,
};
use katana_entity::leader_values::LeaderContext;
use katana_entity::model::Model;
use katana_entity::tables::Alignment;
use katana_geometry::alignment::{AlignmentPi, HorizontalAlignment};
use katana_geometry::profile::{ProfilePvi, VerticalAlignment};
use katana_geometry::{
    CurvePolyline2, CurveVertex, Ellipse2, Point2, Polyline2, Segment2, Spline2, Vec2,
};
use katana_ifc::{
    ClassificationRule, ExportInput, ExportOptions, IfcClass, IfcExport, IfcImport, ImportOptions,
    SurfaceInput, UtilityInput, default_classification_rules, is_ifc_path,
    parse_classification_rules, read_ifc, write_ifc, write_ifc_file,
};
use katana_math::Vec3;
use katana_survey::subsurface::{parse_delivery_schema, parse_utility_csv};
use katana_terrain::TinSurface;
use support::*;

fn add(model: &mut Model, geometry: impl Into<Geometry>, layer: &str, heights: &[Option<f64>]) {
    let mut entity = Entity {
        geometry: geometry.into(),
        layer: String::from(layer),
        ..Entity::default()
    };
    if !heights.is_empty() {
        set_heights(&mut entity.properties, heights);
    }
    must(model.entities.add(entity));
}

fn p(x: f64, y: f64) -> Point2 {
    Point2::new(x, y)
}

fn pi(x: f64, y: f64, radius: f64, spiral_in: f64, spiral_out: f64) -> AlignmentPi {
    AlignmentPi {
        point: p(x, y),
        radius,
        spiral_in,
        spiral_out,
    }
}

fn pvi(station: f64, elevation: f64, curve_length: f64) -> ProfilePvi {
    ProfilePvi {
        station,
        elevation,
        curve_length,
    }
}

/// MC01: four PIs in MGA zone 56, a spiralled curve at the first (R 80,
/// transitions of 20 m) turning right and a simple curve at the second (R 60)
/// turning left, stationed from 1000, with a profile of two vertical curves.
fn mc01() -> Alignment {
    Alignment {
        name: String::from("MC01"),
        description: String::new(),
        horizontal: HorizontalAlignment {
            start_station: 1000.0,
            pis: vec![
                pi(333900.0, 6249950.0, 0.0, 0.0, 0.0),
                pi(334000.0, 6250020.0, 80.0, 20.0, 20.0),
                pi(334100.0, 6249990.0, 60.0, 0.0, 0.0),
                pi(334200.0, 6250060.0, 0.0, 0.0, 0.0),
            ],
        },
        vertical: Some(VerticalAlignment {
            pvis: vec![
                pvi(1000.0, 21.5, 0.0),
                pvi(1080.0, 23.0, 60.0),
                pvi(1180.0, 20.0, 80.0),
                pvi(1330.5, 21.0, 0.0),
            ],
        }),
    }
}

/// The drawing the cli scenario makes (docs/ifc.md): the alignment, a kerb
/// line with its levels, a fence, a stormwater pit, a surveyed point and a
/// lot number.
fn scenario() -> Model {
    let mut model = Model::default();
    must(model.alignments.add(mc01()));
    add(
        &mut model,
        Polyline2::new(
            vec![
                p(333990.0, 6250000.0),
                p(334010.0, 6250004.0),
                p(334030.0, 6250008.0),
            ],
            false,
        ),
        "Survey/Kerb",
        &[Some(20.10), Some(20.05), Some(20.00)],
    );
    add(
        &mut model,
        Polyline2::new(vec![p(333990.0, 6250012.0), p(334030.0, 6250015.0)], false),
        "Survey/Fence",
        &[],
    );
    add(
        &mut model,
        PointGeometry {
            position: p(334015.0, 6249995.0),
        },
        "Stormwater/Pits",
        &[],
    );
    add(
        &mut model,
        PointGeometry {
            position: p(334005.0, 6250002.0),
        },
        "Survey/Points",
        &[],
    );
    add(
        &mut model,
        TextGeometry {
            position: p(334000.0, 6250020.0),
            text: String::from("LOT 42"),
            ..TextGeometry::default()
        },
        "Survey/Points",
        &[],
    );
    model
}

fn mga56() -> ExportOptions {
    let mut options = ExportOptions {
        project_name: String::from("Test"),
        ..ExportOptions::default()
    };
    options.georeference.name = String::from("EPSG:7856");
    options.georeference.description = String::from("GDA2020 / MGA zone 56");
    options
}

/// The C++'s aggregate `{model, surfaces, utilities}`.
fn input<'a>(
    model: Option<&'a Model>,
    surfaces: Vec<SurfaceInput<'a>>,
    utilities: Option<UtilityInput>,
) -> ExportInput<'a> {
    ExportInput {
        model,
        surfaces,
        utilities,
        ..ExportInput::default()
    }
}

fn exported(input: &ExportInput<'_>, options: &ExportOptions) -> IfcExport {
    must(write_ifc(input, options))
}

/// Every instance line of a class, in the order written.
fn instances_of(text: &str, entity: &str) -> Vec<String> {
    let needle = format!("={entity}(");
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(found) = text[from..].find(&needle) {
        let at = from + found;
        let start = text[..at].rfind('\n').map_or(0, |newline| newline + 1);
        let end = text[at..]
            .find('\n')
            .map_or(text.len(), |newline| at + newline);
        out.push(String::from(&text[start..end]));
        from = at + 1;
    }
    out
}

/// The top-level arguments of one instance line, as written.
fn arguments_of(line: &str) -> Vec<String> {
    let open = line.find('(').unwrap();
    let close = line.rfind(')').unwrap();
    let mut out = Vec::new();
    let mut current = String::new();
    let mut depth = 0;
    let mut quoted = false;
    for c in line[open + 1..close].chars() {
        if c == '\'' {
            quoted = !quoted;
        } else if !quoted && c == '(' {
            depth += 1;
        } else if !quoted && c == ')' {
            depth -= 1;
        } else if !quoted && depth == 0 && c == ',' {
            out.push(std::mem::take(&mut current));
            continue;
        }
        current.push(c);
    }
    out.push(current);
    out
}

/// `std::stod`: the number a written REAL is.
fn real(argument: &str) -> f64 {
    argument
        .parse()
        .unwrap_or_else(|error| panic!("{argument}: {error}"))
}

fn text_of(value: &PropertyValue) -> String {
    match value {
        PropertyValue::Text(text) => text.clone(),
        other => panic!("not a text: {other:?}"),
    }
}

fn metadata(entity: &Entity, key: &str) -> String {
    text_of(
        entity
            .metadata
            .get(key)
            .unwrap_or_else(|| panic!("no {key}")),
    )
}

fn with_name<'a>(imported: &'a IfcImport, name: &str) -> Option<&'a Entity> {
    imported.entities.iter().find(|entity| {
        matches!(entity.metadata.get("ifc.name"), Some(PropertyValue::Text(text)) if text == name)
    })
}

fn property(entity: &Entity, name: &str) -> String {
    entity
        .properties
        .get(name)
        .map_or_else(|| String::from("<absent>"), property_to_string)
}

fn read_back(text: &str) -> IfcImport {
    must(read_ifc(text, &ImportOptions::default()))
}

fn sample_schedule() -> UtilityInput {
    let lines = must(parse_utility_csv(&read_text(format!(
        "{SAMPLES}/utilities/schedule.csv"
    ))));
    UtilityInput {
        lines,
        source_name: String::from("schedule.csv"),
        ..UtilityInput::default()
    }
}

fn warned(out: &IfcExport, words: &str) -> bool {
    out.warnings.iter().any(|warning| warning.contains(words))
}

/// Every place `needle` starts, overlapping ones too, as the C++'s countOf
/// steps one byte past each.
fn count_of(text: &str, needle: &str) -> usize {
    let mut count = 0;
    let mut from = 0;
    while let Some(found) = text[from..].find(needle) {
        count += 1;
        from += found + 1;
        while !text.is_char_boundary(from) {
            from += 1;
        }
    }
    count
}

// ---- the file --------------------------------------------------------------------

mod ifc_export {
    use super::*;

    #[test]
    fn writes_an_ifc4x3_add2_file_in_the_alignment_based_view() {
        let model = scenario();
        let out = exported(&input(Some(&model), Vec::new(), None), &mga56());
        assert!(out.text.starts_with("ISO-10303-21;\nHEADER;\n"));
        assert!(
            out.text
                .contains("FILE_DESCRIPTION(('ViewDefinition [Alignment-basedView]'),'2;1');")
        );
        assert!(out.text.contains("FILE_SCHEMA(('IFC4X3_ADD2'));"));
        assert!(out.text.ends_with("ENDSEC;\nEND-ISO-10303-21;\n"));
        assert_eq!(instances_of(&out.text, "IFCPROJECT").len(), 1);
        assert_eq!(instances_of(&out.text, "IFCSITE").len(), 1);
        assert_eq!(out.bytes_written, out.text.len());
        // One line per instance, #1 up.
        assert!(out.text.contains(&format!("\n#{}=", out.instances)));
        assert!(!out.text.contains(&format!("\n#{}=", out.instances + 1)));
    }

    // The local origin is the south-west corner of the work rounded down to
    // 100 m: the first PI is the furthest west (333900) and south (6249950),
    // so 333900, 6249900 - and the map conversion says so, unrotated and
    // unscaled.
    #[test]
    fn is_georeferenced_by_a_map_conversion_from_a_local_origin_near_the_work() {
        let model = scenario();
        let out = exported(&input(Some(&model), Vec::new(), None), &mga56());
        let crs = instances_of(&out.text, "IFCPROJECTEDCRS");
        assert_eq!(crs.len(), 1);
        assert_eq!(arguments_of(&crs[0])[0], "'EPSG:7856'");
        assert_eq!(arguments_of(&crs[0])[1], "'GDA2020 / MGA zone 56'");
        let conversion = instances_of(&out.text, "IFCMAPCONVERSION");
        assert_eq!(conversion.len(), 1);
        let args = arguments_of(&conversion[0]);
        assert_eq!(args[2], "333900.");
        assert_eq!(args[3], "6249900.");
        assert_eq!(args[4], "0.");
        assert_eq!(args[5], "1."); // x axis abscissa
        assert_eq!(args[6], "0."); // x axis ordinate
        assert_eq!(args[7], "1."); // scale
        assert!(out.warnings.is_empty(), "{:?}", out.warnings);

        // A chosen origin is used as given.
        let mut options = mga56();
        options.local_origin = Some(Vec3::new(333000.0, 6249000.0, 10.0));
        let chosen = exported(&input(Some(&model), Vec::new(), None), &options);
        let chosen_args = arguments_of(&instances_of(&chosen.text, "IFCMAPCONVERSION")[0]);
        assert_eq!(chosen_args[2], "333000.");
        assert_eq!(chosen_args[4], "10.");
    }

    #[test]
    fn without_an_epsg_code_the_file_is_not_georeferenced_and_says_so() {
        let model = scenario();
        let mut options = ExportOptions::default();
        let none = exported(&input(Some(&model), Vec::new(), None), &options);
        assert!(instances_of(&none.text, "IFCMAPCONVERSION").is_empty());
        assert_eq!(none.warnings.len(), 1);
        assert!(none.warnings[0].contains("no coordinate system"));
        // The coordinates are the project's own: the kerb's first vertex is
        // written where it is.
        assert!(
            none.text
                .contains("IFCCARTESIANPOINT((333990.,6250000.,20.1))")
        );

        options.georeference.name = String::from("GDA2020 / MGA zone 56 (WKT)");
        let named = exported(&input(Some(&model), Vec::new(), None), &options);
        assert!(instances_of(&named.text, "IFCPROJECTEDCRS").is_empty());
        assert_eq!(named.warnings.len(), 1);
        assert!(named.warnings[0].contains("has no EPSG code"));

        // A shift nothing in the file records would move the survey.
        options.local_origin = Some(Vec3::new(333900.0, 6249900.0, 0.0));
        let refused = write_ifc(&input(Some(&model), Vec::new(), None), &options)
            .expect_err("a local origin with no coordinate system is refused");
        assert_eq!(refused.code, ErrorCode::InvalidArgument);
    }

    // Rule 7: the same input is the same file, byte for byte, and the
    // GlobalIds are the project's - the same next time, different for
    // another project.
    #[test]
    fn the_same_input_is_the_same_file_and_global_ids_belong_to_the_project() {
        let model = scenario();
        let same = input(Some(&model), Vec::new(), Some(sample_schedule()));
        let first = exported(&same, &mga56());
        let second = exported(&same, &mga56());
        assert_eq!(first.text, second.text);

        let mut other = mga56();
        other.guid_namespace = String::from("another project");
        let third = exported(&same, &other);
        assert_ne!(first.text, third.text);
        assert_eq!(first.instances, third.instances);
        let project = arguments_of(&instances_of(&first.text, "IFCPROJECT")[0])[0].clone();
        assert!(!third.text.contains(&project));

        // No GlobalId is used twice in a file. A GlobalId is the first
        // argument of what has one: 22 characters of the IFC alphabet, the
        // first 0 to 3.
        let alphabet = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz_$";
        let text = &first.text;
        let mut ids = BTreeSet::new();
        let mut rooted = 0;
        for (at, _) in text.match_indices("=IFC") {
            let open = at + text[at..].find('(').unwrap();
            if !text[open..].starts_with("('") || text.get(open + 24..open + 26) != Some("',") {
                continue;
            }
            let candidate = &text[open + 2..open + 24];
            if candidate.chars().all(|c| alphabet.contains(c))
                && ('0'..='3').contains(&candidate.chars().next().unwrap())
            {
                rooted += 1;
                assert!(ids.insert(String::from(candidate)), "{candidate}");
            }
        }
        assert!(rooted > 100);
    }

    #[test]
    fn write_ifc_file_writes_the_file_whole() {
        let model = scenario();
        let scratch = Scratch::new("ifc_write_ifc_file_writes_the_file_whole");
        let path = scratch.root.join("katana_ifc_export_test.ifc");
        let written = must(write_ifc_file(
            &input(Some(&model), Vec::new(), None),
            &path,
            &mga56(),
        ));
        assert!(written.text.is_empty());
        let text = read_text(&path);
        assert_eq!(text.len(), written.bytes_written);
        assert!(text.contains("FILE_NAME('katana_ifc_export_test.ifc',"));
        let mut partial = path.clone().into_os_string();
        partial.push(".partial");
        assert!(!Path::new(&partial).exists());
        std::fs::remove_file(&path).unwrap();

        let nowhere = write_ifc_file(
            &input(Some(&model), Vec::new(), None),
            Path::new("/no/such/directory/x.ifc"),
            &mga56(),
        )
        .expect_err("a file in no folder cannot be written");
        assert_eq!(nowhere.code, ErrorCode::FileExportFailure);

        assert!(is_ifc_path(Path::new("site.IFC")));
        assert!(is_ifc_path(Path::new("a/b/site.ifc")));
        assert!(!is_ifc_path(Path::new("site.ifcxml")));
        assert!(!is_ifc_path(Path::new("site.dxf")));
    }

    // A name longer than IFC's 255 characters is cut there, between
    // characters, and the cut is said.
    #[test]
    fn a_name_longer_than_ifc_allows_is_cut_and_said() {
        let mut model = Model::default();
        let mut kerb = Entity {
            geometry: Geometry::Polyline(Polyline2::new(vec![p(0.0, 0.0), p(10.0, 0.0)], false)),
            layer: String::from("Survey/Kerb"),
            ..Entity::default()
        };
        // 300 characters, the 255th a two-byte one: the cut must not split it.
        let name = format!("{}\u{e9}{}", "k".repeat(254), "k".repeat(45));
        kerb.properties
            .insert(String::from("point"), PropertyValue::Text(name));
        must(model.entities.add(kerb));
        let out = exported(
            &input(Some(&model), Vec::new(), None),
            &ExportOptions::default(),
        );
        assert!(
            out.text
                .contains(&format!("'{}\\X2\\00E9\\X0\\'", "k".repeat(254)))
        );
        assert!(warned(
            &out,
            "its name is longer than the 255 characters IFC allows"
        ));
    }
}

// ---- alignments ---------------------------------------------------------------------

mod ifc_export_alignment {
    use super::*;

    // The horizontal's design parameters, worked out from the PIs:
    //   tangent bearings atan2(70, 100) = 0.6107259644 and atan2(-30, 100) =
    //   -0.2914567944, so the deflection at each curve is 0.9021827588;
    //   at PI 1, R 80 with 20 m transitions: the circular arc subtends
    //   0.9021827588 - (20 + 20) / (2 x 80) = 0.6521827588, 52.1746207 m, and
    //   it turns right, so its radius is -80;
    //   at PI 2, R 60, no transitions: 60 x 0.9021827588 = 54.1309655 m, left.
    #[test]
    fn the_horizontal_is_the_pi_definitions_design_parameters_closed_by_a_zero_length_segment() {
        let model = scenario();
        let out = exported(&input(Some(&model), Vec::new(), None), &mga56());
        assert_eq!(out.alignments, 1);
        assert_eq!(out.classes["IfcAlignment"], 1);
        let segments = instances_of(&out.text, "IFCALIGNMENTHORIZONTALSEGMENT");
        assert_eq!(segments.len(), 8);
        let types = [
            ".LINE.",
            ".CLOTHOID.",
            ".CIRCULARARC.",
            ".CLOTHOID.",
            ".LINE.",
            ".CIRCULARARC.",
            ".LINE.",
            ".LINE.",
        ];
        for (segment, expected) in segments.iter().zip(types) {
            assert_eq!(arguments_of(segment)[8], expected, "{segment}");
        }
        let deflection = 70.0_f64.atan2(100.0) + 30.0_f64.atan2(100.0);
        let arc1 = arguments_of(&segments[2]);
        near(real(&arc1[4]), -80.0, 1e-12);
        near(real(&arc1[6]), 80.0 * (deflection - 40.0 / 160.0), 1e-6);
        let spiral = arguments_of(&segments[1]);
        near(real(&spiral[4]), 0.0, 1e-12); // from the tangent...
        near(real(&spiral[5]), -80.0, 1e-12); // ...to the curve
        near(real(&spiral[6]), 20.0, 1e-9);
        near(
            real(&arguments_of(&segments[0])[3]),
            70.0_f64.atan2(100.0),
            1e-9,
        );
        let arc2 = arguments_of(&segments[5]);
        near(real(&arc2[4]), 60.0, 1e-12);
        near(real(&arc2[6]), 60.0 * deflection, 1e-6);
        near(
            real(&arguments_of(&segments[4])[3]),
            -(30.0_f64.atan2(100.0)),
            1e-9,
        );
        // Closed by a segment of length 0 at the end, as IFC 4.3 requires.
        assert_eq!(arguments_of(&segments[7])[6], "0.");
        assert_eq!(arguments_of(&segments[7])[0], "'End'");
    }

    // The profile's: grades (23 - 21.5) / 80 = 0.01875, (20 - 23) / 100 =
    // -0.03 and (21 - 20) / 150.5; the first curve (60 m at 1080) starts at
    // 1050, 50 m from the start, at 21.5 + 50 x 0.01875 = 22.4375, and its
    // radius is L / (g2 - g1) = 60 / -0.04875.
    #[test]
    fn the_vertical_is_the_pvi_definitions_design_parameters() {
        let model = scenario();
        let out = exported(&input(Some(&model), Vec::new(), None), &mga56());
        let segments = instances_of(&out.text, "IFCALIGNMENTVERTICALSEGMENT");
        assert_eq!(segments.len(), 6);
        let types = [
            ".CONSTANTGRADIENT.",
            ".PARABOLICARC.",
            ".CONSTANTGRADIENT.",
            ".PARABOLICARC.",
            ".CONSTANTGRADIENT.",
            ".CONSTANTGRADIENT.",
        ];
        for (segment, expected) in segments.iter().zip(types) {
            assert_eq!(arguments_of(segment)[8], expected, "{segment}");
        }
        let first = arguments_of(&segments[0]);
        near(real(&first[2]), 0.0, 1e-12);
        near(real(&first[3]), 50.0, 1e-9);
        near(real(&first[4]), 21.5, 1e-12);
        near(real(&first[5]), 0.01875, 1e-12);
        let curve = arguments_of(&segments[1]);
        near(real(&curve[2]), 50.0, 1e-9);
        near(real(&curve[3]), 60.0, 1e-9);
        near(real(&curve[4]), 22.4375, 1e-9);
        near(real(&curve[6]), -0.03, 1e-12);
        near(real(&curve[7]), 60.0 / -0.04875, 1e-6);
        near(real(&arguments_of(&segments[4])[5]), 1.0 / 150.5, 1e-12);
        assert_eq!(arguments_of(&segments[5])[3], "0.");
    }

    #[test]
    fn stations_are_referents_with_pset_stationing_at_the_start_the_key_points_and_the_end() {
        let model = scenario();
        let out = exported(&input(Some(&model), Vec::new(), None), &mga56());
        let referents = instances_of(&out.text, "IFCREFERENT");
        assert!(!referents.is_empty());
        assert_eq!(arguments_of(&referents[0])[2], "'Start 1000.000'");
        assert_eq!(arguments_of(&referents[0])[7], ".STATION.");
        assert!(
            out.text
                .contains("IFCPROPERTYSINGLEVALUE('Station',$,IFCLENGTHMEASURE(1000.),$)")
        );
        // TS, SC, CS, ST, PC, PT, the two PVCs and PVTs, the start and the
        // end.
        assert_eq!(referents.len(), 12);
        assert_eq!(out.classes["IfcReferent"], 12);
        assert_eq!(arguments_of(&referents[1])[2], "'PVC 1050.000'");
    }
}

// ---- services -----------------------------------------------------------------------

mod ifc_export_utilities {
    use super::*;

    // samples/utilities/schedule.csv: W1 has six points (W1-X1 among them),
    // E1 four, T1 and G1 two each - one graded segment between each pair,
    // 5 + 3 + 1 + 1 = 10. A segment is drawn in 3D where the centre level is
    // known at both ends: W1-2 and W1-3 are centre levels, W1-X1 and W1-4
    // top levels on a 150 mm main (centre 75 mm below), W1-1 and W1-5 have
    // none - so W1-2 to W1-4 is three segments - and all four of E1's are
    // top levels on a 400 mm size: three more. Six.
    #[test]
    fn every_graded_segment_is_one_element_and_only_those_with_levels_are_in3d() {
        let out = exported(&input(None, Vec::new(), Some(sample_schedule())), &mga56());
        assert_eq!(out.services, 4);
        assert_eq!(out.service_segments, 10);
        assert_eq!(out.segments_in_3d, 6);
        assert_eq!(out.located_points, 14);
        assert_eq!(out.classes["IfcDistributionSystem"], 4);
        // W1 and G1 are pipes; E1 is "4 x 100 mm conduits", cable carriers;
        // T1 a cable.
        assert_eq!(out.classes["IfcPipeSegment"], 6);
        assert_eq!(out.classes["IfcCableCarrierSegment"], 3);
        assert_eq!(out.classes["IfcCableSegment"], 1);
        assert_eq!(out.classes["IfcAnnotation"], 14);
        assert!(!out.classes.contains_key("IfcBuildingElementProxy"));
        assert_eq!(instances_of(&out.text, "IFCSWEPTDISKSOLID").len(), 6);

        let mut system_of = std::collections::BTreeMap::new();
        for line in instances_of(&out.text, "IFCDISTRIBUTIONSYSTEM") {
            let args = arguments_of(&line);
            system_of.insert(args[2].clone(), args[6].clone());
        }
        assert_eq!(system_of["'W1'"], ".WATERSUPPLY.");
        assert_eq!(system_of["'E1'"], ".ELECTRICAL.");
        assert_eq!(system_of["'T1'"], ".COMMUNICATION.");
        assert_eq!(system_of["'G1'"], ".GAS.");
    }

    // E1-1 to E1-2: tops 19.60 and 19.55 on a 400 mm size, so centres 19.40
    // and 19.35. Read back, the segment's axis is at those heights, and it
    // carries its grade.
    #[test]
    fn a_segment_carries_its_grade_and_is_drawn_at_the_centre_of_the_service() {
        let out = exported(&input(None, Vec::new(), Some(sample_schedule())), &mga56());
        let back = read_back(&out.text);
        let segment = with_name(&back, "E1 E1-1 to E1-2").expect("the segment is read back");
        assert_eq!(metadata(segment, "ifc.class"), "IfcCableCarrierSegment");
        assert_eq!(metadata(segment, "ifc.predefinedType"), "CONDUITSEGMENT");
        let heights = heights_of(&segment.properties, 2);
        let (Some(start), Some(end)) = (heights[0], heights[1]) else {
            panic!("the segment has no heights: {heights:?}");
        };
        near(start, 19.40, 1e-9);
        near(end, 19.35, 1e-9);
        assert_eq!(
            property(segment, "AS5488_QualityLevel/QualityLevel"),
            "QL-B"
        );
        assert_eq!(
            property(segment, "AS5488_QualityLevel/ClaimSupported"),
            "true"
        );
        assert_eq!(
            property(segment, "Pset_Uncertainty/UncertaintyBasis"),
            "MEASUREMENT"
        );
        assert_eq!(property(segment, "Classification/AS 5488.1-2019"), "QL-B");
        assert_eq!(property(segment, "System"), "E1");

        // G1 is from the records: QL-D, in plan only - no level was invented.
        let records = with_name(&back, "G1 G1-1 to G1-2").expect("G1 is read back");
        assert_eq!(
            property(records, "AS5488_QualityLevel/QualityLevel"),
            "QL-D"
        );
        assert_eq!(
            property(records, "Pset_Uncertainty/UncertaintyBasis"),
            "ESTIMATE"
        );
        assert!(heights_of(&records.properties, 2)[0].is_none());
    }

    // With the delivery schema, the schedule's own values go in a property
    // set named for it - its title's words joined by '_', "Utility Delivery
    // Schema" in the test data - in its order, a listed value as the
    // enumeration it is from - and a value the schema does not list
    // (E-0001's "In service") as written, not corrected. E-0001's two points
    // are 18 m apart, beyond the 10 m a detected path keeps QL-B for: it
    // claims QL-B and attains QL-C.
    #[test]
    fn the_delivery_schemas_attributes_are_written_as_the_schedule_wrote_them() {
        let lines = must(parse_utility_csv(&read_text(format!(
            "{SAMPLES}/utilities/schedule_tfnsw.csv"
        ))));
        let schema = must(parse_delivery_schema(&read_text(format!(
            "{TEST_DATA}/delivery_schema.csv"
        ))));
        let utilities = UtilityInput {
            lines,
            schema: Some(Arc::new(schema)),
            ..UtilityInput::default()
        };
        let out = exported(&input(None, Vec::new(), Some(utilities)), &mga56());

        assert!(out.text.contains(
            "IFCPROPERTYENUMERATION('AssetStatus',(IFCLABEL('In Service'),IFCLABEL('Disused')),$)"
        ));
        assert!(out.text.contains(
            "IFCPROPERTYSINGLEVALUE('AssetStatus','Asset Status',IFCLABEL('In service'),$)"
        ));
        assert!(out.text.contains(
            "IFCPROPERTYENUMERATEDVALUE('AssetTypeCode','Asset Type Code',(IFCLABEL('E')),"
        ));
        assert!(out.text.contains("'Utility_Delivery_Schema'"));
        // E-0001's asset type code E is electricity in AS 5488.2-2019 Table
        // A.4, the standard the codes are from, whichever schema carried them.
        assert!(
            out.text
                .contains("IFCCLASSIFICATION('Standards Australia','2019',$,'AS 5488.2-2019',")
        );
        assert!(
            out.text
                .contains("IFCCLASSIFICATIONREFERENCE($,'E','electricity',")
        );

        let back = read_back(&out.text);
        let conduit = with_name(&back, "E-0001 E1 to E2").expect("the conduit is read back");
        assert_eq!(metadata(conduit, "ifc.class"), "IfcCableCarrierSegment");
        assert_eq!(
            property(conduit, "AS5488_QualityLevel/QualityLevel"),
            "QL-C"
        );
        assert_eq!(
            property(conduit, "AS5488_QualityLevel/QualityLevelClaimed"),
            "QL-B"
        );
        assert_eq!(
            property(conduit, "AS5488_QualityLevel/ClaimSupported"),
            "false"
        );
        assert_eq!(
            property(conduit, "Utility_Delivery_Schema/AssetStatus"),
            "In service"
        );
        assert_eq!(
            property(conduit, "Pset_ConstructionOccurence/InstallationDate"),
            "2011-02-14"
        );
        // Top of encasement 20.40 - 0.70 = 19.70, the centre 50 mm below on a
        // 100 mm size.
        let heights = heights_of(&conduit.properties, 2);
        let Some(start) = heights[0] else {
            panic!("the conduit has no start height");
        };
        near(start, 19.65, 1e-9);
    }
}

// ---- the drawing --------------------------------------------------------------------

fn drainage_string(name: &str, vertices: Vec<Point2>, pipes: usize) -> Entity {
    let mut line = Entity {
        geometry: Geometry::Polyline(Polyline2::new(vertices, false)),
        layer: String::from("Drainage"),
        ..Entity::default()
    };
    line.metadata.insert(
        String::from("12d.element"),
        PropertyValue::Text(String::from("string drainage")),
    );
    line.metadata.insert(
        String::from("12d.name"),
        PropertyValue::Text(String::from(name)),
    );
    for i in 1..=pipes {
        line.properties
            .insert(format!("pipe.{i}.diameter"), PropertyValue::Real(0.375));
    }
    line
}

fn drainage_pit(name: &str, at: Point2) -> Entity {
    let mut pit = Entity {
        geometry: Geometry::Point(PointGeometry { position: at }),
        layer: String::from("Drainage"),
        ..Entity::default()
    };
    pit.metadata.insert(
        String::from("12d.element"),
        PropertyValue::Text(String::from("drainage pit")),
    );
    pit.metadata.insert(
        String::from("12d.name"),
        PropertyValue::Text(String::from(name)),
    );
    pit.properties.insert(
        String::from("pit.type"),
        PropertyValue::Text(String::from("Junction pit")),
    );
    pit
}

fn text_property(entity: &mut Entity, name: &str, value: &str) {
    entity
        .properties
        .insert(String::from(name), PropertyValue::Text(String::from(value)));
}

/// What UTILITY DRAW puts on a run and a point: the drawing's record of the
/// grading (docs/subsurface_utilities.md, "What the grading found is on the
/// entities"), written here by hand from that table.
fn drawn_run(
    layer: &str,
    line: &str,
    utility: &str,
    level: &str,
    vertices: Vec<Point2>,
    configuration: &str,
) -> Entity {
    let mut run = Entity {
        geometry: Geometry::Polyline(Polyline2::new(vertices, false)),
        layer: String::from(layer),
        ..Entity::default()
    };
    text_property(&mut run, "utility.line", line);
    text_property(&mut run, "utility.type", utility);
    text_property(&mut run, "utility.quality_level", level);
    text_property(&mut run, "utility.from", &format!("{line}-1"));
    text_property(&mut run, "utility.to", &format!("{line}-2"));
    run.properties
        .insert(String::from("utility.length"), PropertyValue::Real(20.0));
    if !configuration.is_empty() {
        text_property(&mut run, "utility.configuration", configuration);
    }
    run
}

fn drawn_point(
    layer: &str,
    line: &str,
    utility: &str,
    vertex: &str,
    level: &str,
    at: Point2,
) -> Entity {
    let mut point = Entity {
        geometry: Geometry::Point(PointGeometry { position: at }),
        layer: String::from(layer),
        ..Entity::default()
    };
    text_property(&mut point, "utility.line", line);
    text_property(&mut point, "utility.type", utility);
    text_property(&mut point, "utility.vertex", vertex);
    text_property(&mut point, "utility.quality_level", level);
    text_property(&mut point, "utility.method", "EML");
    point
}

mod ifc_export_drawing {
    use super::*;

    #[test]
    fn entities_are_the_classes_their_layers_name_and_none_is_a_proxy() {
        let model = scenario();
        let out = exported(&input(Some(&model), Vec::new(), None), &mga56());
        assert_eq!(out.entities_written, 5);
        assert_eq!(out.entities_skipped, 0);
        assert_eq!(out.classes["IfcKerb"], 1);
        assert_eq!(out.classes["IfcRailing"], 1);
        assert_eq!(out.classes["IfcDistributionChamberElement"], 1);
        assert_eq!(out.classes["IfcAnnotation"], 2);
        assert!(!out.classes.contains_key("IfcBuildingElementProxy"));
        assert!(!out.text.contains("IFCBUILDINGELEMENTPROXY"));
        // The kerb's levels make its Axis 3D; the fence has none, so a 2D
        // FootPrint, not a line at height 0.
        assert_eq!(
            arguments_of(&instances_of(&out.text, "IFCRAILING")[0])[8],
            ".FENCE."
        );
        assert!(out.text.contains("IFCCARTESIANPOINT((90.,100.,20.1))"));
        assert!(out.text.contains("IFCCARTESIANPOINT((90.,112.))"));
        assert!(out.text.contains("IFCTEXTLITERALWITHEXTENT('LOT 42',"));
        assert!(!out.text.contains("IFCTEXTLITERAL(")); // deprecated in 4.3
        // Each entity keeps its layer, as a presentation layer.
        assert!(
            out.text
                .contains("IFCPRESENTATIONLAYERASSIGNMENT('Survey/Kerb',")
        );
    }

    // The drawing system's kinds (docs/drawing.md) keep their curves, as an
    // arc does (docs/ifc.md): a curve polyline an IfcIndexedPolyCurve with
    // an IfcArcIndex through the true middle of each arc, an elliptical arc
    // an IfcEllipse trimmed by parameter, a spline a (rational) B-spline with
    // knots - and a kerb drawn with an arc is still a kerb.
    #[test]
    fn the_drawing_systems_curves_stay_curves_and_a_curved_kerb_is_still_a_kerb() {
        let mut model = scenario();
        // Local (the scenario's origin is 333900, 6249900): east from
        // (100,120) to (110,120) at 20, then a semicircle north to (110,130)
        // at 21, bulging east through (115,125) - its middle, at the mean
        // 20.5.
        let vertex = |x: f64, y: f64, bulge: f64, height: f64| CurveVertex {
            position: p(x, y),
            bulge,
            height: Some(height),
        };
        add(
            &mut model,
            CurvePolyline2 {
                vertices: vec![
                    vertex(334000.0, 6250020.0, 0.0, 20.0),
                    vertex(334010.0, 6250020.0, 1.0, 20.0),
                    vertex(334010.0, 6250030.0, 0.0, 21.0),
                ],
                closed: false,
            },
            "Survey/Kerb",
            &[],
        );
        // Half an ellipse, semi-axes 4 east and 2, from eccentric anomaly 0.
        add(
            &mut model,
            Ellipse2 {
                center: p(334020.0, 6250020.0),
                major_axis: Vec2::new(4.0, 0.0),
                ratio: 0.5,
                start_parameter: 0.0,
                sweep: 3.0,
            },
            "Survey/Detail",
            &[],
        );
        // A rational quadratic: three control points, clamped knots, a middle
        // weight of a half.
        let spline = Spline2 {
            degree: 2,
            control_points: vec![
                p(334020.0, 6250040.0),
                p(334025.0, 6250045.0),
                p(334030.0, 6250040.0),
            ],
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            weights: vec![1.0, 0.5, 1.0],
            ..Spline2::default()
        };
        add(&mut model, spline, "Survey/Detail", &[]);

        let out = exported(&input(Some(&model), Vec::new(), None), &mga56());
        assert_eq!(out.entities_written, 8);
        assert_eq!(out.entities_skipped, 0);
        assert_eq!(
            out.classes["IfcKerb"], 2,
            "the curved kerb fell to an annotation"
        );

        let polycurves = instances_of(&out.text, "IFCINDEXEDPOLYCURVE");
        assert!(
            polycurves
                .iter()
                .any(|line| line.contains("IFCLINEINDEX((1,2)),IFCARCINDEX((2,4,3))")),
            "no polycurve of a line then an arc"
        );
        let lists = instances_of(&out.text, "IFCCARTESIANPOINTLIST3D");
        assert!(
            lists.iter().any(|line| line
                .contains("((100.,120.,20.),(110.,120.,20.),(110.,130.,21.),(115.,125.,20.5))")),
            "the arc's middle is ON it, at its ends' mean height"
        );

        let trimmed = instances_of(&out.text, "IFCTRIMMEDCURVE");
        assert_eq!(trimmed.len(), 1);
        assert!(
            trimmed[0].contains("(IFCPARAMETERVALUE(0.)),(IFCPARAMETERVALUE(3.)),.T.,.PARAMETER."),
            "{}",
            trimmed[0]
        );
        let ellipses = instances_of(&out.text, "IFCELLIPSE");
        assert_eq!(ellipses.len(), 1);
        assert!(ellipses[0].contains(",4.,2.)"), "{}", ellipses[0]);

        let splines = instances_of(&out.text, "IFCRATIONALBSPLINECURVEWITHKNOTS");
        assert_eq!(splines.len(), 1);
        let args = arguments_of(&splines[0]);
        assert_eq!(args.len(), 9, "{}", splines[0]);
        assert_eq!(args[0], "2");
        assert_eq!(
            args[5], "(3,3)",
            "each distinct knot once, with its multiplicity"
        );
        assert_eq!(args[6], "(0.,1.)");
        assert_eq!(args[8], "(1.,0.5,1.)");
    }

    #[test]
    fn a_rules_file_may_name_the_drawing_systems_kinds() {
        let rules = must(parse_classification_rules(
            "rule,words,kinds,class,predefined_type\n\
             kerb,KERB,CurvePolyline;Spline,IfcKerb,\n\
             pit,PIT,ellipse,IfcDistributionChamberElement,INSPECTIONCHAMBER\n",
        ));
        assert_eq!(rules.len(), 2);
        assert_eq!(
            rules[0].kinds,
            vec![EntityType::CurvePolyline, EntityType::Spline]
        );
        assert_eq!(rules[1].kinds, vec![EntityType::Ellipse]);
    }

    #[test]
    fn selection_and_options_limit_what_is_written() {
        let model = scenario();
        let mut options = mga56();
        options.layers = vec![String::from("Survey/Kerb")];
        options.export_alignments = false;
        let kerb_only = exported(&input(Some(&model), Vec::new(), None), &options);
        assert_eq!(kerb_only.entities_written, 1);
        assert_eq!(kerb_only.alignments, 0);
        assert!(instances_of(&kerb_only.text, "IFCALIGNMENT").is_empty());

        let mut options = mga56();
        options.export_entities = false;
        let alignment_only = exported(&input(Some(&model), Vec::new(), None), &options);
        assert_eq!(alignment_only.entities_written, 0);
        assert_eq!(alignment_only.alignments, 1);
    }

    #[test]
    fn a_projects_rules_must_name_a_class_the_export_writes_and_say_what_user_defined_is() {
        let model = scenario();
        let rule = |target: IfcClass, words: &str| ClassificationRule {
            name: String::from("x"),
            words: vec![String::from(words)],
            kinds: Vec::new(),
            target,
            system: String::new(),
        };
        let mut options = mga56();
        options.rules = vec![rule(IfcClass::new("IfcKerbStone", "", ""), "KERB")];
        let refused = write_ifc(&input(Some(&model), Vec::new(), None), &options)
            .expect_err("a class the export does not write is refused");
        assert_eq!(refused.code, ErrorCode::InvalidArgument);

        options.rules = vec![rule(IfcClass::new("IfcKerb", "USERDEFINED", ""), "KERB")];
        let refused = write_ifc(&input(Some(&model), Vec::new(), None), &options)
            .expect_err("USERDEFINED without an object type is refused");
        assert_eq!(refused.code, ErrorCode::InvalidArgument);

        // A proxy only when a project asks for one, by name.
        let mut anything = rule(
            IfcClass::new("IfcBuildingElementProxy", "NOTDEFINED", ""),
            "SURVEY",
        );
        anything.name = String::from("anything");
        options.rules = vec![anything];
        let asked = exported(&input(Some(&model), Vec::new(), None), &options);
        assert_eq!(asked.classes["IfcBuildingElementProxy"], 4);
    }

    // A drainage string from a .12da archive: pipes pit to pit, the pits
    // chambers, one system. Pipe 1 runs 19.00 to 18.80 (flow_direction 1:
    // upstream at the start), a 375 mm pipe, so its axis is 0.1875 above:
    // 19.1875 to 18.9875.
    #[test]
    fn an_archives_drainage_string_is_pipes_pit_to_pit_and_its_pits_chambers() {
        let mut model = Model::default();
        let mut line = drainage_string(
            "SW01",
            vec![p(1000.0, 2000.0), p(1030.0, 2000.0), p(1060.0, 2010.0)],
            0,
        );
        let real_property = |line: &mut Entity, name: &str, value: f64| {
            line.properties
                .insert(String::from(name), PropertyValue::Real(value));
        };
        real_property(&mut line, "flow_direction", 1.0);
        real_property(&mut line, "pipe.1.diameter", 0.375);
        real_property(&mut line, "pipe.1.us_level", 19.0);
        real_property(&mut line, "pipe.1.ds_level", 18.8);
        text_property(&mut line, "pipe.1.name", "P1");
        real_property(&mut line, "pipe.2.diameter", 0.375);
        real_property(&mut line, "pipe.2.us_level", 18.75);
        real_property(&mut line, "pipe.2.ds_level", 18.5);
        must(model.entities.add(line));
        for (at, pit_type) in [
            (p(1000.0, 2000.0), "KIP"),
            (p(1030.0, 2000.0), "Junction pit"),
            (p(1060.0, 2010.0), "Headwall"),
        ] {
            let mut pit = drainage_pit("SW01", at);
            text_property(&mut pit, "pit.type", pit_type);
            pit.properties
                .insert(String::from("pit.diameter"), PropertyValue::Real(1.05));
            pit.properties
                .insert(String::from(ELEVATION_PROPERTY), PropertyValue::Real(20.5));
            must(model.entities.add(pit));
        }
        let out = exported(
            &input(Some(&model), Vec::new(), None),
            &ExportOptions::default(),
        );
        assert_eq!(out.classes["IfcPipeSegment"], 2);
        assert_eq!(out.classes["IfcDistributionChamberElement"], 3);
        assert_eq!(out.classes["IfcDistributionSystem"], 1);
        assert!(out.text.contains(".STORMWATER.)"));
        assert!(out.text.contains(".USERDEFINED.)")); // the headwall
        assert!(out.text.contains("'HEADWALL'"));
        assert!(
            out.text
                .contains("IFCCARTESIANPOINT((1000.,2000.,19.1875))")
        );
        assert!(
            out.text
                .contains("IFCCARTESIANPOINT((1030.,2000.,18.9875))")
        );

        let back = read_back(&out.text);
        let pipe = with_name(&back, "P1").expect("the pipe is read back");
        assert_eq!(
            property(pipe, "Pset_PipeSegmentTypeCommon/NominalDiameter"),
            "0.375"
        );
        assert_eq!(
            property(pipe, "Pset_PipeSegmentOccurrence/InvertElevation"),
            "19"
        );
        assert_eq!(property(pipe, "System"), "SW01");
    }

    // A .12da archive names a string's pits by its header, and two strings
    // may share a name: each pit is written once, with the string it stands
    // on.
    #[test]
    fn two_drainage_strings_of_one_name_write_each_pit_once() {
        let mut model = Model::default();
        must(
            model
                .entities
                .add(drainage_string("SW", vec![p(0.0, 0.0), p(30.0, 0.0)], 1)),
        );
        must(
            model
                .entities
                .add(drainage_string("SW", vec![p(0.0, 50.0), p(30.0, 50.0)], 1)),
        );
        for at in [p(0.0, 0.0), p(30.0, 0.0), p(0.0, 50.0), p(30.0, 50.0)] {
            must(model.entities.add(drainage_pit("SW", at)));
        }
        let out = exported(
            &input(Some(&model), Vec::new(), None),
            &ExportOptions::default(),
        );
        assert_eq!(out.classes["IfcDistributionChamberElement"], 4);
        assert_eq!(out.classes["IfcPipeSegment"], 2);
        assert_eq!(out.classes["IfcDistributionSystem"], 2);
        assert_eq!(out.entities_written, 6);
    }

    // A drainage string with nothing to draw is not written, and is
    // accounted for as not written - not claimed as "one pipe along its
    // line".
    #[test]
    fn a_drainage_string_with_nothing_to_draw_is_skipped_and_says_so() {
        let mut model = Model::default();
        must(model.entities.add(drainage_string(
            "SW",
            vec![p(0.0, 0.0), p(0.000001, 0.0), p(0.000002, 0.0)],
            0,
        )));
        let out = exported(
            &input(Some(&model), Vec::new(), None),
            &ExportOptions::default(),
        );
        assert!(!out.classes.contains_key("IfcPipeSegment"));
        assert!(!out.classes.contains_key("IfcDistributionSystem")); // no system of nothing
        assert_eq!(out.entities_written, 0);
        assert_eq!(out.entities_skipped, 1);
        assert_eq!(out.tally.len(), 1);
        assert_eq!(out.tally[0].entity, "");
        assert_eq!(out.tally[0].why, "nothing to draw");
        assert!(warned(&out, "no extent to write; it is not written"));
    }

    // A services plan drawn from a schedule is laid out a layer per type and
    // quality level; it goes out as the services it is. Worked out from the
    // schedule export's own rules (classify_utility_run): an electricity run
    // whose configuration says conduits is an IfcCableCarrierSegment
    // CONDUITSEGMENT, as the schedule's E1 is (ifc_export_utilities above),
    // not the cable its layer's words would make it; water is an
    // IfcPipeSegment RIGIDSEGMENT. One system a service - E1 over its QL-B
    // and QL-C layers is one - and W1 drawn under two prefixes is two
    // services. Each run and point is classified by its level in
    // AS 5488.1-2019: four levels used, four references, one standard.
    #[test]
    fn a_drawn_services_plan_goes_out_by_service_as_the_schedule_would_class_it() {
        let mut model = Model::default();
        let conduits = "4 x 100 mm conduits";
        for entity in [
            drawn_run(
                "utilities/electricity/QL-B",
                "E1",
                "electricity",
                "QL-B",
                vec![p(0.0, 0.0), p(9.0, 0.0)],
                conduits,
            ),
            drawn_run(
                "utilities/electricity/QL-C",
                "E1",
                "electricity",
                "QL-C",
                vec![p(9.0, 0.0), p(27.0, 0.0)],
                conduits,
            ),
            drawn_point(
                "utilities/electricity/points",
                "E1",
                "electricity",
                "E1-1",
                "QL-B",
                p(0.0, 0.0),
            ),
            drawn_point(
                "utilities/electricity/points",
                "E1",
                "electricity",
                "E1-2",
                "QL-C",
                p(27.0, 0.0),
            ),
            drawn_run(
                "utilities/water/QL-A",
                "W1",
                "water",
                "QL-A",
                vec![p(0.0, 5.0), p(20.0, 5.0)],
                "",
            ),
            drawn_point(
                "utilities/water/points",
                "W1",
                "water",
                "W1-1",
                "QL-A",
                p(0.0, 5.0),
            ),
            drawn_point(
                "utilities/water/points",
                "W1",
                "water",
                "W1-2",
                "QL-A",
                p(20.0, 5.0),
            ),
            drawn_run(
                "site b/water/QL-D",
                "W1",
                "water",
                "QL-D",
                vec![p(0.0, 50.0), p(20.0, 50.0)],
                "",
            ),
            drawn_point(
                "site b/water/points",
                "W1",
                "water",
                "W1-1",
                "QL-D",
                p(0.0, 50.0),
            ),
        ] {
            must(model.entities.add(entity));
        }
        let out = exported(
            &input(Some(&model), Vec::new(), None),
            &ExportOptions::default(),
        );
        assert_eq!(out.classes["IfcCableCarrierSegment"], 2);
        assert!(!out.classes.contains_key("IfcCableSegment"));
        assert_eq!(out.classes["IfcPipeSegment"], 2);
        assert_eq!(out.classes["IfcAnnotation"], 5);
        assert_eq!(out.classes["IfcDistributionSystem"], 3);
        assert_eq!(out.entities_written, 9);

        let mut systems: Vec<String> = instances_of(&out.text, "IFCDISTRIBUTIONSYSTEM")
            .iter()
            .map(|line| {
                let arguments = arguments_of(line);
                format!("{} {}", arguments[2], arguments[6])
            })
            .collect();
        systems.sort();
        assert_eq!(
            systems,
            [
                "'E1' .ELECTRICAL.",
                "'W1' .WATERSUPPLY.",
                "'W1' .WATERSUPPLY."
            ]
        );
        for line in instances_of(&out.text, "IFCCABLECARRIERSEGMENT") {
            assert_eq!(arguments_of(&line).last().unwrap(), ".CONDUITSEGMENT.");
        }

        assert_eq!(instances_of(&out.text, "IFCCLASSIFICATION").len(), 1);
        let levels: BTreeSet<String> = instances_of(&out.text, "IFCCLASSIFICATIONREFERENCE")
            .iter()
            .map(|line| arguments_of(line)[1].clone())
            .collect();
        assert_eq!(
            levels,
            ["'QL-A'", "'QL-B'", "'QL-C'", "'QL-D'"]
                .iter()
                .map(|level| String::from(*level))
                .collect::<BTreeSet<_>>()
        );
        assert_eq!(
            instances_of(&out.text, "IFCRELASSOCIATESCLASSIFICATION").len(),
            4
        );
        // The grade in the sets the schedule's export writes: one a run, one
        // a point.
        let sets = instances_of(&out.text, "IFCPROPERTYSET");
        let grades = sets
            .iter()
            .filter(|line| line.contains("'AS5488_QualityLevel'"))
            .count();
        let located = sets
            .iter()
            .filter(|line| line.contains("'AS5488_LocatedPoint'"))
            .count();
        assert_eq!(grades, 4);
        assert_eq!(located, 5);

        // The preview says so, by service - named apart from the schedule's
        // "service E1", and by where it was drawn, so W1's two are two rows.
        let e1 = out
            .tally
            .iter()
            .find(|row| {
                row.source == "drawn service E1 in utilities/electricity"
                    && row.entity == "IfcCableCarrierSegment"
            })
            .expect("E1's runs are in the account");
        assert_eq!(e1.predefined_type, "CONDUITSEGMENT");
        assert_eq!(e1.system, "ELECTRICAL");
        assert_eq!(e1.count, 2);
        assert_eq!(e1.why, "a drawn run: configuration \"4 x 100 mm conduits\"");
    }

    // What the review of the drawn plan's export found (8f101e1). Each case
    // is a plan as UTILITY DRAW leaves it, then edited or drawn again as a
    // person would; the class each run should be is classify_utility_run's
    // for its own attributes, as the schedule's export gives it.

    // Two schedules drawn under one prefix, each with an "E1": conduits in
    // one, a direct-buried cable in the other. Each run is its own service's
    // class, each service its own system with its own description, and the
    // clash of ids is said.
    #[test]
    fn two_services_drawn_with_one_line_id_stay_two_services() {
        let mut model = Model::default();
        let mut conduit = drawn_run(
            "utilities/electricity/QL-B",
            "E1",
            "electricity",
            "QL-B",
            vec![p(0.0, 0.0), p(9.0, 0.0)],
            "4 x 100 mm conduits",
        );
        text_property(&mut conduit, "utility.description", "11 kV");
        let mut buried = drawn_run(
            "utilities/electricity/QL-C",
            "E1",
            "electricity",
            "QL-C",
            vec![p(0.0, 40.0), p(9.0, 40.0)],
            "direct buried",
        );
        text_property(&mut buried, "utility.description", "LV street supply");
        must(model.entities.add(conduit));
        must(model.entities.add(buried));
        let out = exported(
            &input(Some(&model), Vec::new(), None),
            &ExportOptions::default(),
        );
        assert_eq!(out.classes["IfcCableCarrierSegment"], 1);
        assert_eq!(out.classes["IfcCableSegment"], 1);
        assert_eq!(out.classes["IfcDistributionSystem"], 2);
        let described: BTreeSet<String> = instances_of(&out.text, "IFCDISTRIBUTIONSYSTEM")
            .iter()
            .map(|line| arguments_of(line)[3].clone())
            .collect();
        assert_eq!(
            described,
            ["'11 kV'", "'LV street supply'"]
                .iter()
                .map(|text| String::from(*text))
                .collect::<BTreeSet<_>>()
        );
        assert!(warned(
            &out,
            "line E1 under utilities/electricity was drawn from services with different \
             attributes"
        ));
    }

    // EXPLODE makes a run's segments lines that keep its layer and
    // properties: they stay in their service, as the class it gives, not a
    // second system classed by the layer's words.
    #[test]
    fn an_exploded_run_stays_in_its_service() {
        let mut model = Model::default();
        let whole = drawn_run(
            "utilities/electricity/QL-B",
            "E1",
            "electricity",
            "QL-B",
            vec![p(0.0, 0.0), p(9.0, 0.0)],
            "4 x 100 mm conduits",
        );
        let mut piece = whole.clone();
        piece.geometry = Geometry::Line(Segment2::new(p(9.0, 0.0), p(27.0, 0.0)));
        piece.layer = String::from("utilities/electricity/QL-C");
        text_property(&mut piece, "utility.quality_level", "QL-C");
        must(model.entities.add(whole));
        must(model.entities.add(piece));
        let out = exported(
            &input(Some(&model), Vec::new(), None),
            &ExportOptions::default(),
        );
        assert_eq!(out.classes["IfcCableCarrierSegment"], 2);
        assert!(!out.classes.contains_key("IfcCableSegment"));
        assert_eq!(out.classes["IfcDistributionSystem"], 1);
    }

    // A located point moved to another layer - an ordinary edit - still
    // belongs to the service it stands on, and one at the service's level is
    // written in 3D there, as the schedule's export places its points.
    #[test]
    fn a_point_moved_to_another_layer_stays_with_the_run_it_stands_on() {
        let mut model = Model::default();
        must(model.entities.add(drawn_run(
            "utilities/water/QL-A",
            "W1",
            "water",
            "QL-A",
            vec![p(0.0, 5.0), p(20.0, 5.0)],
            "",
        )));
        let mut moved = drawn_point(
            "survey/checked",
            "W1",
            "water",
            "W1-2",
            "QL-A",
            p(20.0, 5.0),
        );
        moved.properties.insert(
            String::from("utility.service_level"),
            PropertyValue::Real(19.3),
        );
        must(model.entities.add(moved));
        let out = exported(
            &input(Some(&model), Vec::new(), None),
            &ExportOptions::default(),
        );
        assert_eq!(out.classes["IfcDistributionSystem"], 1);
        let back = read_back(&out.text);
        let point = with_name(&back, "W1-2").expect("the point is read back");
        let Some(height) = heights_of(&point.properties, 1)[0] else {
            panic!("the point has no height");
        };
        near(height, 19.3, 1e-9);
        // The derived level is ServiceLevel: the schedule export's Level is a
        // recorded one, which the drawing does not keep.
        assert_eq!(property(point, "AS5488_LocatedPoint/ServiceLevel"), "19.3");
    }

    // A project's own rule still names the class of what it drew; the
    // defaults give way to the service's class, a project's rules do not.
    #[test]
    fn a_projects_rule_still_classes_a_drawn_run() {
        let mut model = Model::default();
        must(model.entities.add(drawn_run(
            "utilities/gas/QL-D",
            "G1",
            "gas",
            "QL-D",
            vec![p(0.0, 0.0), p(40.0, 0.0)],
            "",
        )));
        let rules = must(parse_classification_rules(
            "rule,words,kinds,class,predefined_type,object_type,system\n\
             gas flexible,GAS,Polyline,IfcPipeSegment,FLEXIBLESEGMENT,,GAS\n",
        ));
        let mut options = ExportOptions {
            rules,
            ..ExportOptions::default()
        };
        options
            .rules
            .extend_from_slice(default_classification_rules());
        let out = exported(&input(Some(&model), Vec::new(), None), &options);
        let pipes = instances_of(&out.text, "IFCPIPESEGMENT");
        assert_eq!(pipes.len(), 1);
        assert_eq!(arguments_of(&pipes[0]).last().unwrap(), ".FLEXIBLESEGMENT.");
        // Without the project's rule, the service's own class.
        let plain = exported(
            &input(Some(&model), Vec::new(), None),
            &ExportOptions::default(),
        );
        let plain_pipes = instances_of(&plain.text, "IFCPIPESEGMENT");
        assert_eq!(plain_pipes.len(), 1);
        assert_eq!(
            arguments_of(&plain_pipes[0]).last().unwrap(),
            ".RIGIDSEGMENT."
        );
    }

    // A level AS 5488 does not have, or none, is not written as the
    // standard's: it is said, and the run goes out unclassified.
    #[test]
    fn a_level_the_standard_does_not_have_is_said_and_not_classified() {
        let mut model = Model::default();
        must(model.entities.add(drawn_run(
            "utilities/water/QL-B",
            "W1",
            "water",
            "QL-E",
            vec![p(0.0, 0.0), p(20.0, 0.0)],
            "",
        )));
        let out = exported(
            &input(Some(&model), Vec::new(), None),
            &ExportOptions::default(),
        );
        assert_eq!(out.classes["IfcPipeSegment"], 1);
        assert!(instances_of(&out.text, "IFCCLASSIFICATIONREFERENCE").is_empty());
        // Not AS 5488's QualityLevel; the entity's own record keeps what it
        // says (Katana_Attributes), as every entity's does.
        assert_eq!(count_of(&out.text, "'QualityLevel'"), 0);
        assert_eq!(count_of(&out.text, "'QL-E'"), 1);
        assert!(
            out.text
                .contains("IFCPROPERTYSINGLEVALUE('utility.quality_level',$,IFCLABEL('QL-E')")
        );
        assert!(warned(
            &out,
            "has quality level \"QL-E\", which is not one of AS 5488.1-2019's"
        ));
    }

    // The plan drawn from a schedule and the schedule itself in one file is
    // the service twice: written, as asked, and said.
    #[test]
    fn a_service_both_drawn_and_given_as_the_schedule_is_said() {
        let mut model = Model::default();
        must(model.entities.add(drawn_run(
            "utilities/water/QL-B",
            "W1",
            "water",
            "QL-B",
            vec![p(334000.0, 6250000.0), p(334010.0, 6250000.0)],
            "",
        )));
        let out = exported(
            &input(Some(&model), Vec::new(), Some(sample_schedule())),
            &mga56(),
        );
        assert!(warned(
            &out,
            "drawn service W1 in utilities/water is also in the schedule given with UTILITIES"
        ));
    }
}

mod ifc_export_surfaces {
    use super::*;

    #[test]
    fn a_surface_is_terrain_as_a_triangulated_face_set() {
        let surface = must(TinSurface::create(
            vec![
                Vec3::new(0.0, 0.0, 10.0),
                Vec3::new(10.0, 0.0, 11.0),
                Vec3::new(0.0, 10.0, 12.0),
                Vec3::new(10.0, 10.0, 13.0),
            ],
            vec![[0, 1, 3], [0, 3, 2]],
            Vec::new(),
        ));
        let out = exported(
            &input(
                None,
                vec![SurfaceInput {
                    name: String::from("Ground"),
                    surface: Some(&surface),
                }],
                None,
            ),
            &ExportOptions::default(),
        );
        assert_eq!(out.surfaces, 1);
        assert_eq!(out.classes["IfcGeographicElement"], 1);
        assert!(out.text.contains(".TERRAIN.)"));
        assert_eq!(instances_of(&out.text, "IFCTRIANGULATEDFACESET").len(), 1);

        let back = read_back(&out.text);
        assert_eq!(back.surfaces.len(), 1);
        assert_eq!(back.surfaces[0].name, "Ground");
        assert_eq!(back.surfaces[0].surface.vertex_count(), 4);
        assert_eq!(back.surfaces[0].surface.triangle_count(), 2);
    }
}

// ---- leaders ------------------------------------------------------------------------

/// A model holding one point at (30, 40) at RL 25, and the point's id.
struct LeaderScene {
    model: Model,
    point: EntityId,
}

fn point_with_level() -> LeaderScene {
    let mut model = Model::default();
    let mut point = Entity {
        geometry: Geometry::Point(PointGeometry {
            position: p(30.0, 40.0),
        }),
        layer: String::from("Survey/Points"),
        ..Entity::default()
    };
    point
        .properties
        .insert(String::from("elevation"), PropertyValue::Real(25.0));
    must(model.entities.add(point));
    let point = model.entities.ids()[0];
    LeaderScene { model, point }
}

/// A leader whose tip is on the scene's point.
fn leader_on(scene: &LeaderScene, text: &str, smart: bool) -> LeaderGeometry {
    LeaderGeometry {
        vertices: vec![p(30.0, 40.0), p(35.0, 45.0)],
        tip_ref: AnchorRef {
            entity: scene.point,
            point: AnchorPoint::Position,
            ..AnchorRef::default()
        },
        text: String::from(text),
        fields: smart,
        ..LeaderGeometry::default()
    }
}

fn add_leader(scene: &mut LeaderScene, shape: LeaderGeometry) {
    must(scene.model.entities.add(Entity {
        geometry: Geometry::Leader(shape),
        ..Entity::default()
    }));
}

/// The drawing's ground as the cad suite's fake has it: z = 10 + 0.1 x +
/// 0.2 y over the square 0..100 x 0..100, and nothing outside it.
fn ground_context() -> LeaderContext {
    LeaderContext {
        surfaces: vec![String::from("ground")],
        surface_level: Some(Arc::new(|name: &str, at: Point2| {
            let square = 0.0..=100.0;
            if name != "ground" || !square.contains(&at.x) || !square.contains(&at.y) {
                return None;
            }
            Some(10.0 + 0.1 * at.x + 0.2 * at.y)
        })),
        ..LeaderContext::default()
    }
}

fn text_literals_in(out: &IfcExport) -> usize {
    instances_of(&out.text, "IFCTEXTLITERALWITHEXTENT").len()
}

mod ifc_export_leaders {
    use super::*;

    // A smart leader's `text` is its TEMPLATE; what it says is the note read
    // off its target (entity::leader_note), which the plan, LIST and the DXF
    // write. The IFC annotation must say the same, not "RL {rl:.3f}". By
    // hand: the point is at RL 25, and `{rl:.3f}` writes 25 to three
    // decimals, "25.000".
    #[test]
    fn a_smart_leader_is_written_as_the_note_it_says_not_its_template() {
        let mut scene = point_with_level();
        let shape = leader_on(&scene, "RL {rl:.3f}", true);
        add_leader(&mut scene, shape);
        let out = exported(
            &input(Some(&scene.model), Vec::new(), None),
            &ExportOptions::default(),
        );
        assert_eq!(text_literals_in(&out), 1);
        assert!(out.text.contains("IFCTEXTLITERALWITHEXTENT('RL 25.000',"));
        assert!(!out.text.contains("{rl"), "the template reached the file");
    }

    // The drawing's surface comes in with the front end's context. The
    // ground at (30, 40) is 10 + 0.1 * 30 + 0.2 * 40 = 21, and the point at
    // RL 25 is 25 - 21 = 4 above it, which {dsurface.ground:+.3f} writes
    // "+4.000" (the sign always shown). The second line is a line break in
    // the literal: ISO 10303-21 writes U+000A as \X2\000A\X0\. Given no
    // context, the lines that name the surface are dropped (nothing is made
    // up), so the leader writes no text at all, and certainly not the
    // template.
    #[test]
    fn a_leader_that_reads_a_surface_says_it_when_the_front_end_hands_the_surface_over() {
        let mut scene = point_with_level();
        let shape = leader_on(
            &scene,
            "GL {surface.ground:.3f}\n{dsurface.ground:+.3f}",
            true,
        );
        add_leader(&mut scene, shape);

        let with = ExportOptions {
            leader_context: ground_context(),
            ..ExportOptions::default()
        };
        let said = exported(&input(Some(&scene.model), Vec::new(), None), &with);
        assert_eq!(text_literals_in(&said), 1);
        assert!(
            said.text
                .contains("IFCTEXTLITERALWITHEXTENT('GL 21.000\\X2\\000A\\X0\\+4.000',")
        );

        let alone = exported(
            &input(Some(&scene.model), Vec::new(), None),
            &ExportOptions::default(),
        );
        assert_eq!(text_literals_in(&alone), 0);
        assert!(
            !alone.text.contains("surface.ground"),
            "the template reached the file"
        );
    }

    // A tip off the surface leaves the value out, and with it the line:
    // absent is not zero. (150, 40) is outside the 0..100 square, so there is
    // no ground there and the note is empty.
    #[test]
    fn a_leader_whose_tip_is_off_the_surface_writes_no_note_rather_than_a_figure() {
        let mut scene = point_with_level();
        let mut far = Entity {
            geometry: Geometry::Point(PointGeometry {
                position: p(150.0, 40.0),
            }),
            layer: String::from("Survey/Points"),
            ..Entity::default()
        };
        far.properties
            .insert(String::from("elevation"), PropertyValue::Real(25.0));
        must(scene.model.entities.add(far));
        let mut shape = leader_on(&scene, "{dsurface.ground:+.3f}", true);
        shape.vertices = vec![p(150.0, 40.0), p(155.0, 45.0)];
        shape.tip_ref.entity = *scene.model.entities.ids().last().unwrap();
        add_leader(&mut scene, shape);
        let options = ExportOptions {
            leader_context: ground_context(),
            ..ExportOptions::default()
        };
        let out = exported(&input(Some(&scene.model), Vec::new(), None), &options);
        assert_eq!(text_literals_in(&out), 0);
    }

    // A leader made from a label style has no text of its own: its note is
    // the style's template, read live. It used to write no annotation text
    // at all. By hand: the style's `IL {prop.invert:.2f}` on a point whose
    // invert is 10.25 is "IL 10.25".
    #[test]
    fn a_leader_made_from_a_label_style_writes_the_styles_note() {
        let mut model = Model::default();
        let mut pit = Entity {
            geometry: Geometry::Point(PointGeometry {
                position: p(30.0, 40.0),
            }),
            layer: String::from("Stormwater/Pits"),
            ..Entity::default()
        };
        pit.properties
            .insert(String::from("invert"), PropertyValue::Real(10.25));
        must(model.entities.add(pit));
        let point = model.entities.ids()[0];
        let mut scene = LeaderScene { model, point };
        must(scene.model.label_styles.add(LabelStyle {
            name: String::from("Invert"),
            kind: LabelKind::Point,
            text: String::from("IL {prop.invert:.2f}"),
            ..LabelStyle::default()
        }));
        let mut shape = leader_on(&scene, "", false);
        shape.label_style = String::from("Invert");
        add_leader(&mut scene, shape);
        let out = exported(
            &input(Some(&scene.model), Vec::new(), None),
            &ExportOptions::default(),
        );
        assert_eq!(text_literals_in(&out), 1);
        assert!(out.text.contains("IFCTEXTLITERALWITHEXTENT('IL 10.25',"));
    }

    // A plain leader is its text, braces and all (leader_note); an empty one
    // is a bare arrow with no annotation text; a smart one whose target is
    // gone says nothing, since its template is not what it says.
    #[test]
    fn a_plain_leader_keeps_its_words_and_a_smart_one_whose_target_is_gone_writes_none() {
        let mut scene = point_with_level();
        let plain = leader_on(&scene, "LOT {42}", false);
        add_leader(&mut scene, plain);
        let bare = leader_on(&scene, "", false);
        add_leader(&mut scene, bare);
        let mut orphan = leader_on(&scene, "RL {rl:.3f}", true);
        orphan.tip_ref.entity = scene.point + 100; // no such entity
        add_leader(&mut scene, orphan);
        let out = exported(
            &input(Some(&scene.model), Vec::new(), None),
            &ExportOptions::default(),
        );
        assert_eq!(text_literals_in(&out), 1);
        assert!(out.text.contains("IFCTEXTLITERALWITHEXTENT('LOT {42}',"));
        assert!(!out.text.contains("{rl"));
    }
}
