//! The twin of tests/survey/test_subsurface_io.cpp: the schedule and design
//! readers, and the investigation, clearance and verification reports.

mod subsurface_support;

use katana_core::Result;
use katana_survey::coordinate::Coordinate2;
use katana_survey::subsurface::*;
use subsurface_support::{double_eq, near};

// Two services. W1 is a 150 mm water main: two detections 8 m apart and a
// pothole 5 m on. G1 is a gas service known only from the records. The
// header is in a mix of letter cases and aliases, as a spreadsheet export is.
const SCHEDULE: &str = concat!(
    "\u{feff}",
    "Line,Point_ID,Easting,Northing,Location Method,RL,Level_Ref,Surface,H_Unc,V_Unc,QL,Path,",
    "Type,Owner,Material,Diameter_mm,Status,Description\n",
    "# located 2026-09-20\n",
    "W1,P1,1000.0,5000.0,EML,,,,0.10,,QL-B,,water,\"Water Co, North\",DICL,150,live,\n",
    "W1,P2,1008.0,5000.0,EML,,,,0.10,,QL-B,,water,,,,,\n",
    "G1,R1,990.0,4990.0,records,,,,,,QL-D,assumed,gas,,,,,\"from 1987 plan\"\n",
    "W1,P3,1013.0,5000.0,pothole,99.10,centre,100.00,0.02,0.02,QL-A,,,,,150,,\n",
    "\n",
    "G1,R2,990.0,5010.0,records,,,,,,QL-D,,,,,,,\n",
);

fn schedule() -> Vec<UtilityLine> {
    parse_utility_csv(SCHEDULE).unwrap_or_else(|error| panic!("{}", error.describe()))
}

fn ok<T>(result: Result<T>) -> T {
    result.unwrap_or_else(|error| panic!("{}", error.describe()))
}

mod subsurface_utility_csv {
    use super::*;

    #[test]
    fn reads_lines_by_name_in_order_of_first_appearance() {
        let lines = schedule();
        assert_eq!(lines.len(), 2);

        let water = &lines[0];
        assert_eq!(water.id, "W1");
        assert_eq!(water.attributes.r#type, UtilityType::Water);
        assert_eq!(water.attributes.owner, "Water Co, North");
        assert_eq!(water.attributes.material, "DICL");
        double_eq(water.attributes.diameter, 0.150);
        assert_eq!(water.attributes.status, UtilityStatus::InService);
        assert_eq!(water.vertices.len(), 3);
        assert_eq!(water.vertices[1].id, "P2");
        double_eq(water.vertices[1].position.easting, 1008.0);
        double_eq(water.vertices[1].position.northing, 5000.0);
        assert_eq!(
            water.vertices[0].evidence.method,
            LocationMethod::ElectromagneticLocation
        );
        assert!(water.vertices[0].level.is_none()); // absent is not zero
        assert_eq!(water.vertices[2].level_reference, LevelReference::Centre);
        assert_eq!(water.vertices[2].level, Some(99.10));
        assert_eq!(water.vertices[2].surface_level, Some(100.0));
        assert_eq!(water.vertices[2].claimed, Some(QualityLevel::A));
        assert!(water.path_evidence.is_empty());

        let gas = &lines[1];
        assert_eq!(gas.attributes.description, "from 1987 plan");
        assert_eq!(gas.path_evidence.len(), 1);
        assert_eq!(gas.path_evidence[0], PathEvidence::Assumed);
    }

    // What is read is kept as it was written too, under the header it was
    // written under, so that a deliverable written back out (the IFC export's
    // delivery property set) carries the schedule's own values rather than
    // Katana's reading of them.
    #[test]
    fn keeps_each_interpreted_cell_as_written_under_its_header() {
        let lines = schedule();
        let written = &lines[0].attributes.written;
        assert_eq!(written["Line"], "W1");
        assert_eq!(written["Type"], "water");
        assert_eq!(written["Status"], "live");
        assert_eq!(written["Owner"], "Water Co, North");
        assert_eq!(written["Diameter_mm"], "150");
        assert!(!written.contains_key("Description")); // W1 has none

        let pothole = &lines[0].vertices[2].written;
        assert_eq!(pothole["Point_ID"], "P3");
        assert_eq!(pothole["Location Method"], "pothole");
        assert_eq!(pothole["RL"], "99.10"); // not 99.1
        assert_eq!(pothole["Level_Ref"], "centre");
        assert!(!lines[0].vertices[0].written.contains_key("RL")); // absent stays absent
    }

    #[test]
    fn refuses_what_it_would_otherwise_have_to_guess() {
        let code = |text: &str| match parse_utility_csv(text) {
            Ok(_) => String::from("ok"),
            Err(error) => error.describe(),
        };
        let header = "line,point,easting,northing,method";

        // A misspelt column is not read past.
        assert!(
            code(&format!("{header},survace\nW,1,0,0,EML,1\n"))
                .contains("unknown column \"survace\"")
        );
        // X and Y are not accepted for easting and northing: which is which differs
        // between conventions.
        assert!(code("line,point,x,y,method\n").contains("unknown column \"x\""));
        assert!(code("line,point,easting,northing\n").contains("\"method\" is missing"));
        assert!(code(&format!("{header}\nW,1,0,0,dowsing\n")).contains("not a location method"));
        assert!(code(&format!("{header}\nW,1,0,0\n")).contains("4 fields where the header has 5"));
        assert!(code(&format!("{header}\nW,1,0,0,EML\nW,1,1,1,EML\n")).contains("appears twice"));
        assert!(
            code(&format!(
                "{header},type\nW,1,0,0,EML,water\nW,2,1,1,EML,gas\n"
            ))
            .contains("has type \"water\" at line 2 and \"gas\" here [line 3]")
        );
        assert!(
            code(&format!("{header},diameter_mm\nW,1,0,0,EML,0\n")).contains("positive number")
        );
        assert!(code(&format!("{header}\nW,\"1,0,0,EML\n")).contains("unbalanced quotes"));
        assert!(code("# nothing\n").contains("no header row"));
    }

    #[test]
    fn a_column_named_elsewhere_is_the_column_a_header_of_that_name_would_be() {
        // By name and by alias, compared as a header row is: without case,
        // blanks, '-' or '_'.
        for (name, column) in [
            ("type", "type"),
            ("Service Type", "type"),
            ("LocateMethod", "method"),
            ("locate-method", "method"),
            ("DIAMETER_MM", "diameter_mm"),
            ("AssetFeature", "AssetFeature"),
        ] {
            let found = utility_csv_column_named(name);
            assert!(found.is_some(), "{name}");
            assert_eq!(found.unwrap().name, column, "{name}");
        }
        // And it is the reader's own reading: a header of that name reads.
        assert!(
            parse_utility_csv(
                "line,point,easting,northing,locate-method\nW1,1,0,0,EML\nW1,2,1,0,EML\n"
            )
            .is_ok()
        );
        assert!(utility_csv_column_named("colour").is_none());
        assert!(utility_csv_column_named("").is_none());
    }

    #[test]
    fn a_path_reads_its_words_and_the_field_words_for_them() {
        assert_eq!(
            parse_path_evidence("detected"),
            Some(PathEvidence::Detected)
        );
        assert_eq!(parse_path_evidence("Traced"), Some(PathEvidence::Detected));
        assert_eq!(parse_path_evidence("EXPOSED"), Some(PathEvidence::Exposed));
        assert_eq!(parse_path_evidence("trench"), Some(PathEvidence::Exposed));
        assert_eq!(parse_path_evidence("assumed"), Some(PathEvidence::Assumed));
        assert_eq!(parse_path_evidence("inferred"), Some(PathEvidence::Assumed));
        // Each word it writes reads back as itself.
        for evidence in [
            PathEvidence::Detected,
            PathEvidence::Exposed,
            PathEvidence::Assumed,
        ] {
            assert_eq!(parse_path_evidence(to_string(evidence)), Some(evidence));
        }
        assert!(parse_path_evidence("dug").is_none());
        assert!(parse_path_evidence("").is_none());
    }

    #[test]
    fn reads_a_design_centre_line() {
        let design = ok(parse_design_csv(
            "Easting,Northing,Level\n0,0,10\n0,10,\n",
            String::from("stage 1"),
        ));
        assert_eq!(design.id, "stage 1");
        assert_eq!(design.vertices.len(), 2);
        assert_eq!(design.vertices[0].level, Some(10.0));
        double_eq(design.vertices[1].position.northing, 10.0);
        assert!(design.vertices[1].level.is_none());
    }

    // ---- a schedule in the TfNSW Utility Schema's attribute names ------------------

    #[test]
    fn reads_the_tfnsw_attribute_names() {
        let lines = ok(parse_utility_csv(concat!(
            "point,easting,northing,surface,AssetIdentifier,AssetTypeCode,AssetFeature,AssetOwner,",
            "AssetStatus,Size,Configuration,DepthLocation,Depth,QualityLevel,LocateMethod,Notes\n",
            "1,0,0,,D-1,D,Culvert,Private,Disused,1200 x 900,Single,Top Row Invert,1.5,",
            "Quality Level A,Potholing,dug 2026/09/20\n",
            "2,0,5,,D-1,D,Culvert,Private,Disused,1200 x 900,Single,Other,1.4,Unknown,Survey,\n",
        )));
        assert_eq!(lines.len(), 1);
        let culvert = &lines[0];
        assert_eq!(culvert.id, "D-1");
        assert_eq!(culvert.attributes.r#type, UtilityType::Stormwater);
        assert_eq!(culvert.attributes.status, UtilityStatus::Disused);
        assert_eq!(culvert.attributes.owner, "Private");
        // 1200 x 900: the larger side, as an inside dimension.
        double_eq(culvert.attributes.diameter, 1.2);
        assert!(culvert.attributes.diameter_is_inside);
        assert_eq!(culvert.attributes.fields["AssetFeature"], "Culvert");

        let first = &culvert.vertices[0];
        assert_eq!(first.level_reference, LevelReference::Invert);
        assert_eq!(first.depth, Some(1.5));
        assert!(first.level.is_none());
        assert_eq!(first.claimed, Some(QualityLevel::A));
        assert_eq!(
            first.evidence.method,
            LocationMethod::NonDestructiveExcavation
        );
        assert_eq!(first.fields["Notes"], "dug 2026/09/20");

        let second = &culvert.vertices[1];
        assert_eq!(second.level_reference, LevelReference::Unknown);
        assert!(second.claimed.is_none()); // "Unknown" claims nothing
        assert_eq!(second.evidence.method, LocationMethod::SurfaceFeature);

        // Cover from a depth alone: to the invert 1.5, less the 1.2 m inside
        // height, is 0.3 m to the inside top - and says the wall is not in it.
        // The second vertex's depth is to an unknown point on the culvert.
        let cover = ok(depth_of_cover(culvert, None, &GradingSettings::default()));
        assert!(cover[0].cover.is_some());
        near(cover[0].cover.unwrap(), 0.3, 1e-12);
        assert!(cover[1].cover.is_none());
        assert_eq!(cover[1].note, "the level's place on the service is unknown");
    }

    #[test]
    fn inside_cover_is_said_to_be_to_the_inside_top() {
        let a = UtilityVertex {
            id: String::from("a"),
            depth: Some(1.5),
            level_reference: LevelReference::Invert,
            evidence: PositionEvidence {
                method: LocationMethod::NonDestructiveExcavation,
                horizontal_uncertainty: Some(0.02),
                vertical_uncertainty: Some(0.02),
                has_level: true,
            },
            ..Default::default()
        };
        let mut b = a.clone();
        b.id = String::from("b");
        b.position = Coordinate2 {
            northing: 0.0,
            easting: 1.0,
        };
        let line = UtilityLine {
            id: String::from("W"),
            attributes: UtilityAttributes {
                diameter: 0.3,
                diameter_is_inside: true,
                ..Default::default()
            },
            vertices: vec![a, b],
            ..Default::default()
        };
        let cover = ok(depth_of_cover(&line, None, &GradingSettings::default()));
        near(cover[0].cover.unwrap(), 1.2, 1e-12);
        assert!(cover[0].note.contains("inside top"), "{}", cover[0].note);
    }

    #[test]
    fn the_schema_names_of_types_methods_and_levels_parse() {
        assert_eq!(
            parse_quality_level("Quality Level B"),
            Some(QualityLevel::B)
        );
        assert!(parse_quality_level("Unknown").is_none());
        assert_eq!(
            parse_location_method("Electronic Detection"),
            Some(LocationMethod::ElectromagneticLocation)
        );
        assert_eq!(
            parse_location_method("Archive Drawings and Plans"),
            Some(LocationMethod::Records)
        );
        assert_eq!(
            parse_location_method("Geographic Information System"),
            Some(LocationMethod::Records)
        );
        assert_eq!(
            parse_location_method("Unknown"),
            Some(LocationMethod::Unknown)
        );
        assert_eq!(
            maximum_quality_level(LocationMethod::Unknown),
            QualityLevel::D
        );
        assert_eq!(
            parse_level_reference("Top of Concrete Encasement"),
            Some(LevelReference::Top)
        );
        assert_eq!(
            parse_level_reference("Plastic Cover Protection Encountered"),
            Some(LevelReference::Top)
        );
        assert_eq!(parse_level_reference("Obvert"), Some(LevelReference::Top));
        assert_eq!(
            parse_level_reference("Other"),
            Some(LevelReference::Unknown)
        );
        assert_eq!(
            parse_utility_type("Fire Service"),
            Some(UtilityType::FireService)
        );
        assert_eq!(parse_utility_type("F"), Some(UtilityType::FireService));
        assert_eq!(
            parse_utility_type("ITS"),
            Some(UtilityType::IntelligentTransport)
        );
        assert_eq!(parse_utility_type("Petroleum"), Some(UtilityType::Fuel));
        assert_eq!(
            parse_utility_type("Not Specified"),
            Some(UtilityType::Unknown)
        );
        assert!(parse_utility_type("X").is_none());
        assert_eq!(
            parse_utility_status("Disused"),
            Some(UtilityStatus::Disused)
        );
    }

    #[test]
    fn a_services_schema_attributes_must_agree_across_its_rows() {
        let lines = parse_utility_csv(
            "point,easting,northing,method,AssetIdentifier,AssetFeature\n\
             1,0,0,EML,C-1,Pit\n2,0,5,EML,C-1,Pole\n",
        );
        let error = lines.unwrap_err();
        assert!(
            error
                .message
                .contains("AssetFeature \"Pit\" at line 2 and \"Pole\" here"),
            "{}",
            error.describe()
        );
        let size =
            parse_utility_csv("point,easting,northing,method,line,Size\n1,0,0,EML,C,large\n");
        assert!(size.unwrap_err().message.contains("W x H millimetres"));
    }
}

mod subsurface_report {
    use super::*;

    #[test]
    fn the_investigation_report_ends_with_what_to_act_on() {
        let lines = schedule();
        let text = ok(render_investigation_report(
            &lines,
            &GradingSettings::default(),
            Some(0.9),
        ));

        assert!(text.contains("2 lines, 5 vertices"), "{text}");
        // P3: centre 99.10 on a 150 mm main -> top 99.175, cover 0.825.
        assert!(text.contains("cover 0.825"), "{text}");
        assert!(
            text.contains("W1 / P3: cover 0.825 m is below the minimum 0.900 m"),
            "{text}"
        );
        // P2 -> P3 is 5 m between a detection and an exposure: QL-B.
        assert!(text.contains("P2 -> P3  5.000 m  QL-B"), "{text}");
        assert!(
            text.contains("G1: not recorded: owner, status, material, size"),
            "{text}"
        );
        assert!(
            text.contains("water                      0.000      13.000"),
            "{text}"
        );
    }

    #[test]
    fn a_non_graded_claim_is_a_finding() {
        let lines = ok(parse_utility_csv(
            "line,point,easting,northing,method,ql\nW,1,0,0,GPR,QL-A\nW,2,0,5,GPR,QL-B\n",
        ));
        let report = ok(render_investigation_report(
            &lines,
            &GradingSettings::default(),
            None,
        ));
        // No uncertainty given: not assumed to be good.
        assert!(
            report.contains(
                "W / 1: claimed QL-A, the ground penetrating radar evidence supports QL-C"
            ),
            "{report}"
        );
    }

    #[test]
    fn clearance_and_verification_reports_name_what_to_do_next() {
        let lines = schedule();
        let design = DesignAlignment {
            id: String::from("stage 1"),
            vertices: vec![
                DesignVertex {
                    position: Coordinate2 {
                        northing: 4995.0,
                        easting: 1010.0,
                    },
                    level: None,
                },
                DesignVertex {
                    position: Coordinate2 {
                        northing: 5005.0,
                        easting: 1010.0,
                    },
                    level: None,
                },
            ],
            ..Default::default()
        };
        let requirement = ClearanceRequirement::default();
        let results = ok(check_clearance(
            &design,
            &lines,
            &requirement,
            &GradingSettings::default(),
        ));
        let clearance = render_clearance_report(&design, &results, &requirement);
        // The works cross W1's P2-P3 in plan with no levels: a conflict. P1-P2
        // ends 2 m short of them (QL-B: 2 - 0.075 - 0.3 >= 0.3), and G1 runs
        // north-south at easting 990, 20 m off: clear even at QL-D's margin.
        assert!(clearance.contains("2 services, 3 segments"), "{clearance}");
        assert!(
            clearance.contains("Segments: 1 conflict, 0 unconfirmed, 0 within tolerance, 2 clear"),
            "{clearance}"
        );
        assert!(clearance.contains("W1 P2 -> P3"), "{clearance}");
        assert!(!clearance.contains("W1 P1 -> P2"), "{clearance}"); // clear: not listed

        let verification = render_verification_report(
            &verify_detections(&lines, &GradingSettings::default()),
            &GradingSettings::default(),
        );
        assert!(
            verification.contains("nothing was verified"),
            "{verification}"
        );
    }

    #[test]
    fn a_claim_along_the_whole_asset_is_tested_segment_by_segment() {
        // Both ends claim and are QL-B; 18 m between them is past the 10 m spacing.
        let lines = ok(parse_utility_csv(
            "line,point,easting,northing,method,h_unc,ql\n\
             E,1,0,0,GPR,0.2,QL-B\nE,2,18,0,GPR,0.2,QL-B\n",
        ));
        let report = ok(render_investigation_report(
            &lines,
            &GradingSettings::default(),
            None,
        ));
        assert!(
            report.contains(
                "E: 18.000 m is claimed better than it grades: 1 -> 2 claimed QL-B, grades QL-C"
            ),
            "{report}"
        );
    }

    #[test]
    fn the_clash_suggested_for_a_delivery_schema() {
        let mut result = ClearanceResult {
            status: ClearanceStatus::Clear,
            ..Default::default()
        };
        assert_eq!(clash_of(&result), Clash::No);
        result.status = ClearanceStatus::Unconfirmed;
        assert_eq!(clash_of(&result), Clash::Unknown);
        result.status = ClearanceStatus::WithinTolerance;
        assert_eq!(clash_of(&result), Clash::Soft);
        result.status = ClearanceStatus::Conflict;
        result.horizontal_gap = 0.1; // apart, but closer than required
        assert_eq!(clash_of(&result), Clash::Soft);
        result.horizontal_gap = -0.2; // overlapping in plan, no levels to separate them
        assert_eq!(clash_of(&result), Clash::Hard);
        result.vertical_gap = Some(0.1); // overlapping in plan, apart in level
        assert_eq!(clash_of(&result), Clash::Soft);
        assert_eq!(to_string(Clash::Hard), "Hard");
    }
}
