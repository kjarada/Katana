//! Twin of tests/ifc/test_front_end.cpp: the verbs' grammar, the files an
//! export names, a project's rules file, what an export accounts for, the
//! extent of an import and the replies - the part of IFC the command line,
//! the window and an agent share.

pub mod support;

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

use katana_core::text::{read_reply_record, split_lines};
use katana_core::{ErrorCode, Result};
use katana_entity::entity::{Entity, EntityType, Geometry, LabelGeometry, PointGeometry};
use katana_entity::model::Model;
use katana_entity::tables::Alignment;
use katana_geometry::alignment::{AlignmentPi, HorizontalAlignment};
use katana_geometry::{Point2, Polyline2};
use katana_ifc::{
    ClassTally, ClassificationRule, ExportArguments, ExportInput, ExportOptions, IfcClass,
    ImportArguments, ImportOptions, SurfaceInput, classify_utility_point, classify_utility_run,
    default_classification_rules, format_classification_rules, format_description,
    format_export_line, format_export_reply, format_import_line, format_import_reply,
    format_info_line, format_rules_line, guid_namespace_for, header_timestamp,
    parse_classification_rules, parse_export_arguments, parse_import_arguments,
    parse_rules_arguments, path_from_utf8, read_export_files, read_export_objects, read_ifc,
    write_default_rules, write_ifc,
};
use katana_math::Vec3;
use katana_survey::subsurface::{UtilityAttributes, UtilityType};
use katana_terrain::TinSurface;
use support::*;

fn refusal(parsed: &Option<Result<ExportArguments>>) -> String {
    match parsed {
        None => String::from("<not an ifc line>"),
        Some(Ok(_)) => String::from("ok"),
        Some(Err(error)) => error.describe(),
    }
}

/// The arguments of a line that must parse.
fn parsed_export(line: &str) -> ExportArguments {
    let parsed = parse_export_arguments(line);
    match parsed {
        Some(Ok(arguments)) => arguments,
        other => panic!("{line}: {}", refusal(&other)),
    }
}

fn parsed_import(line: &str) -> ImportArguments {
    match parse_import_arguments(line) {
        Some(Ok(arguments)) => arguments,
        Some(Err(error)) => panic!("{line}: {}", error.describe()),
        None => panic!("{line}: not an ifc line"),
    }
}

fn p(x: f64, y: f64) -> Point2 {
    Point2::new(x, y)
}

fn kerb_on(layer: &str, from: Point2, to: Point2) -> Entity {
    Entity {
        geometry: Geometry::Polyline(Polyline2::new(vec![from, to], false)),
        layer: String::from(layer),
        ..Entity::default()
    }
}

fn straight(name: &str, from: Point2, to: Point2) -> Alignment {
    let pi = |point: Point2| AlignmentPi {
        point,
        radius: 0.0,
        spiral_in: 0.0,
        spiral_out: 0.0,
    };
    Alignment {
        name: String::from(name),
        description: String::new(),
        horizontal: HorizontalAlignment {
            pis: vec![pi(from), pi(to)],
            start_station: 0.0,
        },
        vertical: None,
    }
}

/// The C++'s `writeIfc(input)`, its options defaulted.
fn written(input: &ExportInput<'_>) -> katana_ifc::IfcExport {
    must(write_ifc(input, &ExportOptions::default()))
}

fn model_input(model: &Model) -> ExportInput<'_> {
    ExportInput {
        model: Some(model),
        ..ExportInput::default()
    }
}

fn lines_of(text: &str) -> Vec<&str> {
    split_lines(text.as_bytes())
        .into_iter()
        .map(|line| std::str::from_utf8(line).unwrap())
        .collect()
}

// ---- the grammar ------------------------------------------------------------------

mod ifc_front_end {
    use super::*;

    #[test]
    fn an_export_line_is_its_path_and_its_options_in_any_case() {
        let arguments = parsed_export(
            "site plan.ifc utilities \"a b.csv\" Schema s.csv RULES r.csv spacing 12.5 NoDrawing",
        );
        assert_eq!(arguments.path, "site plan.ifc"); // up to the first .ifc ending a word
        assert_eq!(arguments.schedule.as_deref(), Some("a b.csv"));
        assert_eq!(arguments.schema.as_deref(), Some("s.csv"));
        assert_eq!(arguments.rules.as_deref(), Some("r.csv"));
        assert_eq!(arguments.spacing, Some(12.5));
        // NODRAWING: the investigation alone - no entities, alignments,
        // surfaces or road model.
        assert!(
            !(arguments.entities
                || arguments.alignments
                || arguments.surfaces
                || arguments.road_model)
        );

        let quoted = parsed_export("  \"C:/jobs/x y.IFC\"  ");
        assert_eq!(quoted.path, "C:/jobs/x y.IFC");
        assert!(quoted.entities && quoted.alignments && quoted.surfaces && quoted.road_model);
        assert!(!(quoted.selected || quoted.preview));
        assert!(quoted.schedule.is_none());
    }

    // Not an .ifc: the line is another exporter's, and is left to it.
    #[test]
    fn a_line_that_names_no_ifc_is_left_to_the_other_formats() {
        assert!(parse_export_arguments("site.dxf").is_none());
        assert!(parse_export_arguments("site.ifcxml").is_none());
        assert!(parse_export_arguments("\"site.dxf\" UTILITIES x.csv").is_none());
        assert!(parse_import_arguments("survey.12da").is_none());
        assert!(parse_import_arguments("").is_none());
    }

    // The refusals, said once here for both front ends: an unknown word
    // names every option the verb takes - NOROADMODEL among them since the
    // road model was added to the export (docs/ifc.md, "Road model").
    #[test]
    fn a_refused_export_line_says_why_as_the_command_line_always_has() {
        assert_eq!(
            refusal(&parse_export_arguments("x.ifc UTILITIS s.csv")),
            "InvalidArgument: \"UTILITIS\" is not an option of EXPORT <file.ifc>: UTILITIES \
             <schedule.csv>, SCHEMA <schema.csv>, RULES <rules.csv>, SPACING <m>, NODRAWING, \
             NOENTITIES, SELECTED, NOALIGNMENTS, NOSURFACES, NOROADMODEL, PREVIEW"
        );
        assert_eq!(
            refusal(&parse_export_arguments("x.ifc SCHEMA s.csv")),
            "InvalidArgument: SCHEMA describes a schedule: give it with UTILITIES"
        );
        assert_eq!(
            refusal(&parse_export_arguments("x.ifc SPACING 0")),
            "InvalidArgument: SPACING takes a positive number of metres"
        );
        assert_eq!(
            refusal(&parse_export_arguments("x.ifc SPACING ten")),
            "InvalidArgument: SPACING takes a positive number of metres"
        );
        assert_eq!(
            refusal(&parse_export_arguments("x.ifc UTILITIES \"never closed")),
            "InvalidArgument: a quoted path is never closed"
        );
        // A keyword with no value is not taken for one.
        assert!(
            refusal(&parse_export_arguments("x.ifc UTILITIES")).contains("\"UTILITIES\" is not")
        );

        let local = parsed_import("x.ifc local");
        assert!(local.local);
        let wrong = parse_import_arguments("x.ifc LOCALLY")
            .expect("an ifc line")
            .expect_err("LOCALLY is not a word of IMPORT");
        assert_eq!(
            wrong.describe(),
            "InvalidArgument: \"LOCALLY\" is not an option of IMPORT <file.ifc>: LOCAL, \
             NOALIGNMENTS, NOELEMENTS, NOSURFACES, TOLERANCE <m>, TAKECRS, KEEPCRS"
        );
    }

    // Every choice the IFC dialogs offer is a word of the line, so that a
    // typed line, a script and an agent can ask for anything a dialog can.
    #[test]
    fn every_choice_of_the_dialogs_is_a_word_of_the_line() {
        let exported = parsed_export("x.ifc noalignments NOSURFACES selected Preview");
        assert!(exported.entities);
        assert!(!exported.alignments);
        assert!(!exported.surfaces);
        assert!(exported.selected);
        assert!(exported.preview);
        let no_entities = parsed_export("x.ifc NOENTITIES");
        assert!(!no_entities.entities);
        assert!(no_entities.alignments && no_entities.surfaces);

        let imported = parsed_import("x.ifc NOALIGNMENTS nosurfaces TOLERANCE 0.005 KeepCrs");
        assert!(!imported.alignments);
        assert!(imported.elements);
        assert!(!imported.surfaces);
        assert_eq!(imported.tolerance, Some(0.005));
        assert_eq!(imported.take_coordinate_system, Some(false));
        let take = parsed_import("x.ifc TAKECRS NOELEMENTS");
        assert_eq!(take.take_coordinate_system, Some(true));
        assert!(!take.elements);
        // Neither: the front end's own way (the window asks, a typed line
        // says how).
        let plain = parsed_import("x.ifc");
        assert!(plain.take_coordinate_system.is_none());
    }

    // NOROADMODEL leaves the road model out and nothing else; NODRAWING
    // leaves it out with the rest of the drawing; given twice, it is refused;
    // and a line leaving out only the road model says so and reads back as
    // it was asked.
    #[test]
    fn no_road_model_is_a_word_and_no_drawing_leaves_the_road_model_out_too() {
        let parsed = parsed_export("x.ifc NoRoadModel");
        assert!(!parsed.road_model);
        assert!(parsed.entities && parsed.alignments && parsed.surfaces);

        let no_drawing = parsed_export("x.ifc NODRAWING UTILITIES s.csv");
        assert!(!no_drawing.road_model);

        assert_eq!(
            refusal(&parse_export_arguments("x.ifc NOROADMODEL noroadmodel")),
            "InvalidArgument: NOROADMODEL is given twice"
        );

        let roadless = ExportArguments {
            path: String::from("x.ifc"),
            road_model: false,
            ..ExportArguments::default()
        };
        let line = must(format_export_line(&roadless));
        assert_eq!(line, "EXPORT \"x.ifc\" NOROADMODEL");
        let back = parsed_export(&line["EXPORT".len()..]);
        assert_eq!(back, roadless);

        // The rest of the drawing left out but not the road model: each word,
        // not NODRAWING, which would leave the road model out too.
        let road_only = ExportArguments {
            path: String::from("x.ifc"),
            entities: false,
            alignments: false,
            surfaces: false,
            ..ExportArguments::default()
        };
        assert_eq!(
            must(format_export_line(&road_only)),
            "EXPORT \"x.ifc\" NOENTITIES NOALIGNMENTS NOSURFACES"
        );
    }

    // What would be contradictory, repeated, or would import nothing is
    // refused by name rather than read one way or the other.
    #[test]
    fn contradictory_or_repeated_words_are_refused() {
        assert_eq!(
            refusal(&parse_export_arguments("x.ifc PREVIEW preview")),
            "InvalidArgument: PREVIEW is given twice"
        );
        assert_eq!(
            refusal(&parse_export_arguments("x.ifc NOENTITIES SELECTED")),
            "InvalidArgument: SELECTED chooses among the entities, and NOENTITIES leaves them out"
        );
        assert_eq!(
            refusal(&parse_export_arguments(
                "x.ifc SELECTED NODRAWING UTILITIES s.csv"
            )),
            "InvalidArgument: SELECTED chooses among the entities, and NODRAWING leaves them out"
        );
        let import_refusal = |line: &str| match parse_import_arguments(line) {
            None => String::from("<not an ifc line>"),
            Some(Ok(_)) => String::from("ok"),
            Some(Err(error)) => error.describe(),
        };
        assert_eq!(
            import_refusal("x.ifc LOCAL local"),
            "InvalidArgument: LOCAL is given twice"
        );
        assert_eq!(
            import_refusal("x.ifc NOALIGNMENTS NOELEMENTS NOSURFACES"),
            "InvalidArgument: NOALIGNMENTS, NOELEMENTS and NOSURFACES together leave nothing to \
             import"
        );
        assert_eq!(
            import_refusal("x.ifc TAKECRS KEEPCRS"),
            "InvalidArgument: TAKECRS and KEEPCRS say opposite things: give one"
        );
        assert_eq!(
            import_refusal("x.ifc LOCAL TAKECRS"),
            "InvalidArgument: LOCAL moves the data out of the file's coordinate system, which \
             TAKECRS would give the project"
        );
        assert_eq!(
            import_refusal("x.ifc TOLERANCE -1"),
            "InvalidArgument: TOLERANCE takes a positive number of metres"
        );
        assert!(import_refusal("x.ifc TOLERANCE").contains("\"TOLERANCE\" is not an option"));
        // LOCAL with KEEPCRS says the same thing twice, and is taken.
        assert_eq!(import_refusal("x.ifc LOCAL KEEPCRS"), "ok");
    }

    // The line a dialog hands the command line reads back as what it was
    // asked, whatever the paths hold - blanks, a ".ifc " inside a folder's
    // name, a name that is an option's word - and a number to its last bit.
    #[test]
    fn the_line_a_dialog_writes_reads_back_as_its_arguments() {
        let everything = ExportArguments {
            path: String::from("C:/jobs/old.ifc files/Site Plan.ifc"),
            schedule: Some(String::from("a b.csv")),
            schema: Some(String::from("NODRAWING")),
            rules: Some(String::from("r.csv")),
            spacing: Some(0.1), // not exact in binary: shortest round trip
            alignments: false,
            selected: true,
            preview: true,
            ..ExportArguments::default()
        };
        // The investigation alone: every part of the drawing left out.
        let alone = ExportArguments {
            path: String::from("x.ifc"),
            schedule: Some(String::from("s.csv")),
            entities: false,
            alignments: false,
            surfaces: false,
            road_model: false,
            ..ExportArguments::default()
        };
        for arguments in [everything, alone.clone(), ExportArguments::default()] {
            let mut asked = arguments;
            if asked.path.is_empty() {
                asked.path = String::from("plain.ifc");
            }
            let line = must(format_export_line(&asked));
            assert!(line.starts_with("EXPORT \""), "{line}");
            let back = parsed_export(&line["EXPORT".len()..]);
            assert_eq!(back, asked, "{line}");
        }
        assert_eq!(
            must(format_export_line(&alone)),
            "EXPORT \"x.ifc\" UTILITIES \"s.csv\" NODRAWING"
        );

        let imported = ImportArguments {
            path: String::from("a b.ifc"),
            local: true,
            elements: false,
            tolerance: Some(0.003),
            take_coordinate_system: Some(false),
            ..ImportArguments::default()
        };
        let line = must(format_import_line(&imported));
        assert_eq!(
            line,
            "IMPORT \"a b.ifc\" LOCAL NOELEMENTS TOLERANCE 0.003 KEEPCRS"
        );
        let back = parsed_import(&line["IMPORT".len()..]);
        assert_eq!(back, imported);

        // A double quote cannot be said inside a quoted word: refused, not
        // written as a line that would read as something else.
        let quote = ExportArguments {
            path: String::from("say \"hi\".ifc"),
            ..ExportArguments::default()
        };
        assert_eq!(
            format_export_line(&quote)
                .expect_err("a quote in a path is refused")
                .code,
            ErrorCode::InvalidArgument
        );
        assert_eq!(must(format_info_line("a b.ifc")), "INFO \"a b.ifc\"");
        assert_eq!(
            must(format_rules_line("my rules.csv")),
            "IFC RULES \"my rules.csv\""
        );
    }

    // IFC RULES <file.csv>: the defaults, written for a project to edit,
    // read back as the defaults they are.
    #[test]
    fn ifc_rules_writes_the_defaults_for_a_project_to_edit() {
        assert!(parse_rules_arguments("OTHER x.csv").is_none());
        assert!(parse_rules_arguments("").is_none());
        let path = parse_rules_arguments(" rules \"a b.csv\"").expect("a rules line");
        assert_eq!(must(path), "a b.csv");
        let extra = parse_rules_arguments("RULES a.csv b.csv")
            .expect("a rules line")
            .expect_err("two files are refused");
        assert_eq!(
            extra.describe(),
            "InvalidArgument: usage: IFC RULES <file.csv>"
        );

        let scratch = Scratch::new("ifc_rules_writes_the_defaults_for_a_project_to_edit");
        let file = scratch.root.join("katana ifc default rules.csv");
        let file = file.to_str().unwrap();
        let reply = must(write_default_rules(file));
        assert_eq!(
            reply,
            format!(
                "ifc rules file=\"katana ifc default rules.csv\" rules={}",
                default_classification_rules().len()
            )
        );
        let back = must(parse_classification_rules(&read_text(file)));
        let defaults = default_classification_rules();
        assert_eq!(back.len(), defaults.len());
        for (read, default) in back.iter().zip(defaults) {
            assert_eq!(read.name, default.name);
            assert_eq!(read.target, default.target);
        }

        let nowhere = scratch.root.join("no such folder").join("r.csv");
        let refused = write_default_rules(nowhere.to_str().unwrap())
            .expect_err("a file in no folder cannot be written");
        assert_eq!(refused.code, ErrorCode::FileExportFailure);
    }

    #[test]
    fn the_global_id_namespace_is_the_project_and_the_timestamp_is_utc() {
        assert_eq!(
            guid_namespace_for("Road 7", "2026-09-01T10:00:00Z"),
            "Road 7/2026-09-01T10:00:00Z"
        );
        // 2026-09-25T16:01:09.750Z, truncated to the second. By hand: 20721
        // days from 1970-01-01 (56 years of which 14 leap, 20454 days, then
        // 267 days into 2026), 20721 x 86400 + 16 h 1 min 9 s = 1790352069 s.
        let when = UNIX_EPOCH + Duration::from_millis(1_790_352_069_750);
        assert_eq!(header_timestamp(when), "2026-09-25T16:01:09");
    }

    // ---- what an export names --------------------------------------------------

    #[test]
    fn the_files_an_export_names_are_read_the_schema_owned_and_the_rules_before_the_defaults() {
        let scratch = Scratch::new(
            "ifc_the_files_an_export_names_are_read_the_schema_owned_and_the_rules_before_the_defaults",
        );
        let mut arguments = ExportArguments {
            path: String::from("x.ifc"),
            schedule: Some(format!("{SAMPLES}/utilities/schedule_tfnsw.csv")),
            schema: Some(format!("{TEST_DATA}/delivery_schema.csv")),
            spacing: Some(20.0),
            rules: Some(scratch.file(
                "katana_ifc_rules_test.csv",
                "rule,words,class\nlight pole,POLE*,IfcColumn\n",
            )),
            ..ExportArguments::default()
        };
        let files = must(read_export_files(&arguments));
        let utilities = files.utilities.as_ref().expect("the schedule is read");
        assert_eq!(utilities.lines.len(), 3);
        assert_eq!(utilities.source_name, "schedule_tfnsw.csv");
        assert_eq!(utilities.grading.maximum_detected_spacing, 20.0);
        assert!(files.schema.is_some());
        // Moved, the schema stays where the input points.
        let moved = files;
        assert!(Arc::ptr_eq(
            moved
                .utilities
                .as_ref()
                .unwrap()
                .schema
                .as_ref()
                .expect("the input points at the schema"),
            moved.schema.as_ref().unwrap()
        ));
        assert_eq!(moved.rules.len(), 1 + default_classification_rules().len());
        assert_eq!(moved.rules[0].name, "light pole");
        assert_eq!(moved.rules[1].name, default_classification_rules()[0].name);

        arguments.schedule = Some(String::from("/no/such/schedule.csv"));
        let missing =
            read_export_files(&arguments).expect_err("a schedule that is not there is refused");
        assert_eq!(missing.code, ErrorCode::NotFound);
        assert!(missing.describe().contains("/no/such/schedule.csv"));

        // A schedule that does not parse is named with the reader's error.
        arguments.schedule = Some(scratch.file("katana_ifc_bad_schedule.csv", "line,point\nW,1\n"));
        arguments.schema = None;
        let bad = read_export_files(&arguments).expect_err("a schedule that does not parse");
        assert!(
            bad.context.contains("katana_ifc_bad_schedule.csv"),
            "{}",
            bad.describe()
        );
    }

    // ---- what the review found -------------------------------------------------

    // A quote opened before a .ifc and never closed is refused, not handed to
    // the other formats' grammar to be written under a name starting '"'.
    #[test]
    fn an_unclosed_quote_before_an_ifc_is_refused() {
        assert_eq!(
            refusal(&parse_export_arguments("\"site plan.ifc")),
            "InvalidArgument: a quoted path is never closed"
        );
        let import = parse_import_arguments("\"site.ifc LOCAL")
            .expect("an ifc line")
            .expect_err("an unclosed quote is refused");
        assert_eq!(
            import.describe(),
            "InvalidArgument: a quoted path is never closed"
        );
        // Not an .ifc: still the other formats' line.
        assert!(parse_export_arguments("\"site.dxf").is_none());
    }

    // katana_cli's arguments arrive in the ANSI code page on Windows, where
    // the "é" of "café" is the one byte E9 - not UTF-8. This module's own
    // conversion threw on such a name, out of a verb nothing catches; it is
    // core's now (core/path_text), which reads those bytes as the narrow name
    // they are.
    #[test]
    fn a_path_that_is_not_utf8_is_the_narrow_name_it_is_and_is_not_thrown_on() {
        let narrow = path_from_utf8(b"caf\xE9.ifc");
        // Whatever character the platform makes of E9, the rest is untouched.
        assert_eq!(narrow.extension(), Some(OsStr::new("ifc")));
        assert!(narrow.file_stem().is_some_and(|stem| !stem.is_empty()));

        // And UTF-8 is still read as UTF-8: "Zürich", its u-umlaut the two
        // bytes C3 BC, is six characters whatever the platform's own encoding.
        let zurich = path_from_utf8("Z\u{fc}rich");
        assert_eq!(zurich.to_str().unwrap().chars().count(), 6);
    }
}

// ---- a project's rules ------------------------------------------------------------

mod ifc_rules_file {
    use super::*;

    #[test]
    fn a_rules_file_is_read_by_its_header_and_each_rule_checked() {
        let text = concat!(
            "\u{feff}# a project's own layers\n",
            "Class,Rule,Words,Kinds,Predefined_Type,System\n",
            "IfcColumn,light pole,\"POLE*; LP\",point,column,\n",
            "ifcpipesegment,drainage line,DRN,Polyline;Line,rigidsegment,stormwater\n",
            "\n",
        );
        let rules = must(parse_classification_rules(text));
        assert_eq!(rules.len(), 2);
        let pole = &rules[0];
        assert_eq!(pole.name, "light pole");
        assert_eq!(pole.words, ["POLE*", "LP"]);
        assert_eq!(pole.kinds, [EntityType::Point]);
        assert_eq!(pole.target, IfcClass::new("IfcColumn", "COLUMN", ""));
        let drain = &rules[1];
        assert_eq!(drain.target.entity, "IfcPipeSegment"); // the schema's spelling
        assert_eq!(drain.system, "STORMWATER");
        assert_eq!(drain.kinds.len(), 2);
    }

    #[test]
    fn what_would_write_an_invalid_file_is_refused_with_its_line() {
        let why = |rows: &str| match parse_classification_rules(&format!(
            "rule,words,kinds,class,predefined_type,object_type\n{rows}"
        )) {
            Ok(_) => String::from("ok"),
            Err(error) => error.describe(),
        };
        assert_eq!(why("pole,POLE,,IfcColumn,COLUMN,\n"), "ok");
        assert!(
            why("pole,POLE,,IfcColumnn,,\n")
                .contains("\"IfcColumnn\" is not a class this export writes")
        );
        assert!(
            why("pole,POLE,,IfcColumn,PILLAR,\n")
                .contains("\"PILLAR\" is not a predefined type of IfcColumn")
        );
        assert!(
            why("bore,BH,,IfcBorehole,NOTDEFINED,\n")
                .contains("IfcBorehole has no predefined type")
        );
        assert!(why("pole,POLE,Tree,IfcColumn,,\n").contains("\"Tree\" is not a kind"));
        assert!(why("pole,,,IfcColumn,,\n").contains("has no words"));
        assert!(why("pole,POLE,,IfcColumn,USERDEFINED,\n").contains("says nothing of what it is"));
        assert_eq!(why("pole,POLE,,IfcColumn,USERDEFINED,LIGHT POLE\n"), "ok");
        assert!(why("pole,POLE,,IfcColumn\n").contains("4 fields where the header has 6"));
        assert!(why("pole,\"POLE,,IfcColumn,,\n").contains("never closed"));
        // The line is the file's own: the header is line 1.
        assert!(why("ok,A,,IfcColumn,,\nbad,B,,IfcNothing,,\n").contains("[line 3]"));

        let no_class = parse_classification_rules("rule,words\npole,POLE\n")
            .expect_err("a file with no class column is refused");
        assert!(no_class.describe().contains("\"class\" is missing"));
        let unknown = parse_classification_rules("rule,words,class,colour\n")
            .expect_err("an unknown column is refused");
        assert!(unknown.describe().contains("unknown column \"colour\""));
    }

    // The defaults written out read back as themselves: the window offers
    // them as the starting point of a project's file.
    #[test]
    fn the_defaults_written_out_read_back_as_themselves() {
        let defaults = default_classification_rules();
        let text = format_classification_rules(defaults);
        let back = must(parse_classification_rules(&text));
        assert_eq!(back.len(), defaults.len());
        for (read, default) in back.iter().zip(defaults) {
            assert_eq!(read.name, default.name);
            assert_eq!(read.words, default.words);
            assert_eq!(read.kinds, default.kinds);
            assert_eq!(read.target, default.target);
            assert_eq!(read.system, default.system);
        }
    }

    // Every class the export can choose on its own names a predefined type
    // the schema has: the check a rules file gets, applied to the defaults
    // and to the service classes (the enumerations are IfcOpenShell's,
    // generated).
    #[test]
    fn every_class_the_export_chooses_is_one_the_schema_has() {
        // The defaults pass the check a file's rules get.
        assert!(
            parse_classification_rules(
                &format_classification_rules(default_classification_rules())
            )
            .is_ok()
        );
        // Every UtilityType, Unknown to Other, in its order.
        let types = [
            UtilityType::Unknown,
            UtilityType::Electricity,
            UtilityType::Telecommunications,
            UtilityType::Gas,
            UtilityType::Water,
            UtilityType::RecycledWater,
            UtilityType::FireService,
            UtilityType::Sewer,
            UtilityType::Stormwater,
            UtilityType::Fuel,
            UtilityType::IntelligentTransport,
            UtilityType::Other,
        ];
        let features = [
            "",
            "Culvert",
            "Conduit",
            "Trough",
            "Optic fibre",
            "Cable",
            "Pipe",
            "Manhole",
            "Valve pit",
            "Meter pit",
            "Sump",
            "Chamber",
            "Pit",
            "Hydrant",
            "Stop valve",
            "Air valve",
            "Scour",
            "Valve",
            "Marker",
            "Pillar",
        ];
        let mut rows = String::from("rule,words,class,predefined_type,object_type\n");
        for utility in types {
            for feature in features {
                let mut service = UtilityAttributes {
                    r#type: utility,
                    ..UtilityAttributes::default()
                };
                service
                    .fields
                    .insert(String::from("AssetFeature"), String::from(feature));
                let run = classify_utility_run(&service).element;
                rows += &format!(
                    "run,X,{},{},{}\n",
                    run.entity, run.predefined_type, run.object_type
                );
                if let Some(point) = classify_utility_point(&service) {
                    rows += &format!(
                        "point,X,{},{},{}\n",
                        point.element.entity,
                        point.element.predefined_type,
                        point.element.object_type
                    );
                }
            }
        }
        if let Err(error) = parse_classification_rules(&rows) {
            panic!("{}", error.describe());
        }
    }

    #[test]
    fn a_comment_is_skipped_whole_and_a_quoted_hash_is_data() {
        // A quote in a comment must not open a field that swallows the rules
        // after it; a quoted field that starts '#' is a rule, not a comment.
        let text = concat!(
            "rule,words,kinds,class,predefined_type,object_type,system\n",
            "# light poles, \"LP or POLE on the lighting layer\n",
            "light pole,LIGHTING,Point,IfcColumn,COLUMN,,\n",
            "  # kerbs surveyed as 12\" strings\n",
            "\"#1 kerb\",KB,,\"IfcKerb\" ,,,\n",
        );
        let rules = must(parse_classification_rules(text));
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].name, "light pole");
        assert_eq!(rules[1].name, "#1 kerb");
        assert_eq!(rules[1].target.entity, "IfcKerb"); // the blank after the quote is not its
        // Anything but blanks between a closing quote and the comma is refused.
        let after = parse_classification_rules("rule,words,class\npole,POLE,\"IfcColumn\"x\n")
            .expect_err("text after a closing quote is refused");
        assert!(
            after
                .describe()
                .contains("text after a quoted field's closing quote")
        );
        assert!(after.describe().contains("[line 2]"));
    }

    // Whatever a rule is called and whatever its words, what the writer
    // writes the reader reads back as it was.
    #[test]
    fn any_rule_written_out_reads_back_as_it_was() {
        let rule =
            |name: &str, words: &[&str], kinds: &[EntityType], target: IfcClass, system: &str| {
                ClassificationRule {
                    name: String::from(name),
                    words: words.iter().map(|word| String::from(*word)).collect(),
                    kinds: kinds.to_vec(),
                    target,
                    system: String::from(system),
                }
            };
        let rules = vec![
            rule(
                "#1 pole",
                &["POLE*", "LP"],
                &[EntityType::Point],
                IfcClass::new("IfcColumn", "COLUMN", ""),
                "",
            ),
            rule(
                "a, \"quoted\" name",
                &["WALL"],
                &[],
                IfcClass::new("IfcWall", "USERDEFINED", "SOUND, WALL"),
                "NOISE",
            ),
            rule(
                " padded ",
                &["FENCE"],
                &[],
                IfcClass::new("IfcRailing", "FENCE", ""),
                "",
            ),
        ];
        let back = must(parse_classification_rules(&format_classification_rules(
            &rules,
        )));
        assert_eq!(back.len(), rules.len());
        for (read, written) in back.iter().zip(&rules) {
            assert_eq!(read.name, written.name);
            assert_eq!(read.words, written.words);
            assert_eq!(read.kinds, written.kinds);
            assert_eq!(read.target, written.target);
            assert_eq!(read.system, written.system);
        }
    }
}

// ---- what an export accounts for ---------------------------------------------------

fn tally(
    source: &str,
    entity: &str,
    predefined_type: &str,
    object_type: &str,
    system: &str,
    why: &str,
    count: usize,
) -> ClassTally {
    ClassTally {
        source: String::from(source),
        entity: String::from(entity),
        predefined_type: String::from(predefined_type),
        object_type: String::from(object_type),
        system: String::from(system),
        why: String::from(why),
        count,
    }
}

mod ifc_export_tally {
    use super::*;

    #[test]
    fn the_report_accounts_for_each_source_by_the_class_it_became_and_why() {
        let mut model = Model::default();
        let kerb = kerb_on("Survey/Kerb", p(0.0, 0.0), p(10.0, 0.0));
        must(model.entities.add(kerb.clone()));
        must(model.entities.add(kerb));
        let label = Entity {
            geometry: Geometry::Label(LabelGeometry {
                style: String::from("Chainage"),
                alignment: String::from("MC01"),
                ..LabelGeometry::default()
            }),
            layer: String::from("Notes"),
            ..Entity::default()
        };
        must(model.entities.add(label));
        let point = Entity {
            geometry: Geometry::Point(PointGeometry {
                position: p(5.0, 5.0),
            }),
            layer: String::from("Notes"),
            ..Entity::default()
        };
        must(model.entities.add(point));

        let out = written(&model_input(&model));
        let expected = [
            tally(
                "layer Notes",
                "",
                "",
                "",
                "",
                "a label: labels are not exported",
                1,
            ),
            tally(
                "layer Notes",
                "IfcAnnotation",
                "SURVEY",
                "",
                "",
                "no rule: a point",
                1,
            ),
            tally(
                "layer Survey/Kerb",
                "IfcKerb",
                "NOTDEFINED",
                "",
                "",
                "rule kerb",
                2,
            ),
        ];
        assert_eq!(out.tally, expected);
    }

    // Every product the report counts is in the account, once, as the class
    // written - the preview is only as true as this. (IfcDistributionSystem
    // is a group, not a product: it is in the classes map and not in the
    // tally.)
    #[test]
    fn every_product_the_report_counts_is_accounted_for_once() {
        let mut model = Model::default();
        must(model.entities.add(kerb_on(
            "Survey/Kerb",
            p(334000.0, 6250000.0),
            p(334010.0, 6250000.0),
        )));
        must(model.entities.add(Entity {
            geometry: Geometry::Point(PointGeometry {
                position: p(334005.0, 6250002.0),
            }),
            layer: String::from("Stormwater/Pits"),
            ..Entity::default()
        }));
        must(model.alignments.add(straight(
            "A",
            p(334000.0, 6250000.0),
            p(334100.0, 6250050.0),
        )));
        let surface = must(TinSurface::create(
            vec![
                Vec3::new(334000.0, 6250000.0, 10.0),
                Vec3::new(334010.0, 6250000.0, 11.0),
                Vec3::new(334000.0, 6250010.0, 12.0),
            ],
            vec![[0, 1, 2]],
            Vec::new(),
        ));
        let arguments = ExportArguments {
            path: String::from("x.ifc"),
            schedule: Some(format!("{SAMPLES}/utilities/schedule.csv")),
            ..ExportArguments::default()
        };
        let files = must(read_export_files(&arguments));

        let out = written(&ExportInput {
            model: Some(&model),
            surfaces: vec![SurfaceInput {
                name: String::from("Ground"),
                surface: Some(&surface),
            }],
            utilities: files.utilities,
            ..ExportInput::default()
        });
        let mut tallied = BTreeMap::new();
        for row in &out.tally {
            if !row.entity.is_empty() {
                *tallied.entry(row.entity.clone()).or_insert(0) += row.count;
            }
        }
        let mut counted = out.classes.clone();
        counted.remove("IfcDistributionSystem");
        assert_eq!(tallied, counted);
        // A located point is in its service's system, as the file groups it.
        let point = out
            .tally
            .iter()
            .find(|row| row.source == "service W1" && row.entity == "IfcAnnotation")
            .expect("W1's located points are in the account");
        assert_eq!(point.system, "WATERSUPPLY");
        assert_eq!(point.count, 6);
    }
}

// ---- the extent of an import ------------------------------------------------------

mod ifc_import_bounds {
    use super::*;

    // An alignment alone has an extent, so LOCAL and the far-apart question
    // treat it as they treat entities.
    #[test]
    fn the_extent_includes_the_alignments_and_surfaces() {
        let mut model = Model::default();
        must(
            model
                .alignments
                .add(straight("A", p(1000.0, 2000.0), p(1100.0, 2050.0))),
        );
        let out = written(&model_input(&model));
        let back = must(read_ifc(&out.text, &ImportOptions::default()));
        assert!(back.entities.is_empty());
        assert!(!back.bounds.empty());
        near(back.bounds.min.x, 1000.0, 1e-6);
        near(back.bounds.max.y, 2050.0, 1e-6);
    }

    // A terrain alone has an extent too.
    #[test]
    fn a_surface_alone_has_an_extent() {
        let surface = must(TinSurface::create(
            vec![
                Vec3::new(500.0, 700.0, 10.0),
                Vec3::new(510.0, 700.0, 11.0),
                Vec3::new(500.0, 720.0, 12.0),
            ],
            vec![[0, 1, 2]],
            Vec::new(),
        ));
        let out = written(&ExportInput {
            surfaces: vec![SurfaceInput {
                name: String::from("Ground"),
                surface: Some(&surface),
            }],
            ..ExportInput::default()
        });
        let back = must(read_ifc(&out.text, &ImportOptions::default()));
        assert_eq!(back.surfaces.len(), 1);
        assert!(!back.bounds.empty());
        near(back.bounds.min.x, 500.0, 1e-9);
        near(back.bounds.min.y, 700.0, 1e-9);
        near(back.bounds.max.x, 510.0, 1e-9);
        near(back.bounds.max.y, 720.0, 1e-9);
    }
}

// ---- the replies ------------------------------------------------------------------

mod ifc_replies {
    use super::*;

    // The reply is records a reader can take back: the head's counts are the
    // report's, and the object records are the tally, row for row - the one
    // reader File > Export IFC's table is filled from, and what an agent
    // reads.
    #[test]
    fn an_exports_object_records_read_back_as_its_tally() {
        let mut model = Model::default();
        must(model.entities.add(kerb_on(
            "Survey/Kerb \"east\"",
            p(334000.0, 6250000.0),
            p(334010.0, 6250000.0),
        )));
        must(model.entities.add(Entity {
            geometry: Geometry::Point(PointGeometry {
                position: p(334005.0, 6250002.0),
            }),
            layer: String::from("Stormwater/Pits"),
            ..Entity::default()
        }));
        let arguments = ExportArguments {
            path: String::from("x.ifc"),
            schedule: Some(format!("{SAMPLES}/utilities/schedule.csv")),
            ..ExportArguments::default()
        };
        let files = must(read_export_files(&arguments));
        let out = written(&ExportInput {
            model: Some(&model),
            utilities: files.utilities,
            ..ExportInput::default()
        });

        let reply = format_export_reply(&out, "site plan.ifc", false);
        let lines = lines_of(&reply);
        assert!(!lines.is_empty());
        for line in &lines {
            assert!(read_reply_record(line).is_some(), "{line}"); // every line is a record
        }
        let head = read_reply_record(lines[0]).expect("the head is a record");
        assert_eq!(head.words, ["ifc", "exported"]);
        assert_eq!(head.value("file"), Some("site plan.ifc"));
        let instances = format!("{}", out.instances);
        assert_eq!(head.value("instances"), Some(instances.as_str()));

        let objects = must(read_export_objects(&reply));
        assert_eq!(objects, out.tally);

        let preview = format_export_reply(&out, "x.ifc", true);
        assert!(preview.starts_with("ifc previewed file=\"x.ifc\" schema=IFC4X3_ADD2 instances="));
        assert!(!preview.contains("bytes=")); // nothing was written

        let broken = read_export_objects("ifc exported file=\"x.ifc\"\nobject count=many")
            .expect_err("a count that is not a number is refused");
        assert_eq!(broken.code, ErrorCode::ParseFailure);
    }

    // An import's reply and INFO's description are records too, the counts
    // the read's own.
    #[test]
    fn an_import_and_a_description_are_records_of_what_was_read() {
        let mut model = Model::default();
        must(model.entities.add(kerb_on(
            "Survey/Kerb",
            p(334000.0, 6250000.0),
            p(334010.0, 6250000.0),
        )));
        must(model.alignments.add(straight(
            "MC 01",
            p(334000.0, 6250000.0),
            p(334100.0, 6250050.0),
        )));
        let out = written(&model_input(&model));
        let read = must(read_ifc(&out.text, &ImportOptions::default()));

        let reply = format_import_reply(&read, "x.ifc", read.entities.len());
        let head = read_reply_record(lines_of(&reply)[0]).expect("the head is a record");
        assert_eq!(head.words, ["ifc", "imported"]);
        let entities = format!("{}", read.entities.len());
        assert_eq!(head.value("entities"), Some(entities.as_str()));
        assert_eq!(head.value("alignments"), Some("1"));
        assert_eq!(head.value("crs"), Some("")); // none: absent, and said as empty, not as zero

        let described = format_description(&read, "x.ifc");
        let mut saw_alignment = false;
        for line in lines_of(&described) {
            let record = read_reply_record(line).unwrap_or_else(|| panic!("{line}"));
            if record.words == ["alignment"] {
                saw_alignment = true;
                assert_eq!(record.value("name"), Some("MC 01"));
                assert_eq!(record.value("pis"), Some("2"));
            }
        }
        assert!(saw_alignment);
        assert!(described.starts_with("ifc described file=\"x.ifc\""));
    }
}
