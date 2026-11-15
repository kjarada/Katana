// The UTILITY verbs (utilities/utility_verbs.rs;
// tests/cad/utilities/test_utility_verbs.cpp) through CommandInterpreter::run,
// as the window's command line, katana_cli, katana_mcp and an agent type them.
//
// REPORT, VERIFY, CLEARANCE and CHECK reply with the report and never touch
// the drawing; CHECK with errors is refused with the whole check in the
// refusal. DRAW is ONE undo step that undo takes back entirely - entities,
// layers and linetypes - and a refused DRAW changes nothing.

mod utility_verbs {
    use katana_cad::CommandInterpreter;
    use katana_cad::utilities::utility_drawing::{keys, utility_type_colour};
    use katana_cad::utilities::utility_verbs::{draw_reply_bounds, utility_verb_help};
    use katana_core::ErrorCode;
    use katana_entity::{Layer, PropertyValue};
    use katana_survey::subsurface::UtilityType;

    use crate::{
        SAMPLES, ScratchFolder, Session, contains, double_eq, first_line, generic, spaced_search,
    };

    fn schedule() -> String {
        format!("\"{SAMPLES}/schedule.csv\"")
    }

    fn design() -> String {
        format!("\"{SAMPLES}/design.csv\"")
    }

    fn scratch(name: &str) -> ScratchFolder {
        ScratchFolder::new(&format!("katana-utility-verbs-{name}"))
    }

    fn entities(session: &Session) -> usize {
        session.document.model().entities.size()
    }

    fn layers(session: &Session) -> Vec<String> {
        session.document.model().layers.names()
    }

    fn has_linetype(session: &Session, name: &str) -> bool {
        session.document.model().linetypes.contains(name)
    }

    #[track_caller]
    fn layer(session: &Session, name: &str) -> Layer {
        match session.document.model().layers.find(name) {
            Some(layer) => layer.clone(),
            None => panic!("no layer {name}"),
        }
    }

    // A schema of the tests' own, in the shape tools/utility_schema_domains.py
    // writes: the real ones are clients' documents and stay out of the
    // repository.
    const SCHEMA: &str = "kind,attribute,value,detail,label\n\
                          schema,Example Utility Schema,0.1,,\n\
                          identifier,AssetId,,,\n\
                          field,AssetId,Alphanumerical,Yes,Asset Id\n\
                          field,Status,Domain List: Status,Yes,Status\n\
                          domain,Status,Live,,\n\
                          domain,Status,Dead,,\n";

    #[test]
    fn report_replies_with_the_report_and_leaves_the_drawing_alone() {
        let mut session = Session::default();
        let report = session.ok(&format!("UTILITY REPORT {} MINCOVER 0.6", schedule()));
        assert!(
            contains(
                &report,
                "AS 5488 subsurface utility investigation: 4 lines, 14 vertices"
            ),
            "{report}"
        );
        assert!(contains(&report, "Minimum cover: 0.600 m"));
        assert!(contains(
            &report,
            "E1 / E1-3: claimed QL-B, the ground penetrating radar evidence supports QL-C"
        ));
        // The front end ends the reply with its own line break.
        assert!(!report.ends_with('\n'));
        assert_eq!(session.steps(), 0);
        assert_eq!(entities(&session), 0);
        // Words in any case; SPACING grades as DRAW's does.
        let spaced = session.ok(&format!("utility report {} spacing 20", schedule()));
        assert!(
            contains(&spaced, "detected spacing <= 20.000 m"),
            "{spaced}"
        );
        assert!(!contains(&spaced, "W1-4 -> W1-5  12.502 m  QL-C"));
    }

    #[test]
    fn verify_compares_the_pothole_with_its_detection() {
        let mut session = Session::default();
        let report = session.ok(&format!("UTILITY VERIFY {}", schedule()));
        assert!(
            spaced_search(
                &report,
                &["W1", "W1-3", "W1-X1", "0.100", "ok", "-0.100", "ok"]
            ),
            "{report}"
        );
        assert_eq!(session.steps(), 0);
    }

    #[test]
    fn clearance_shows_the_crossing_within_tolerance() {
        let mut session = Session::default();
        let report = session.ok(&format!(
            "UTILITY CLEARANCE {} {} WIDTH 0.375",
            schedule(),
            design()
        ));
        assert!(
            spaced_search(
                &report,
                &[
                    "W1 W1-2 -> W1-3",
                    "QL-B",
                    "within tolerance",
                    "-0.263",
                    "0.300",
                    "0.372"
                ]
            ),
            "{report}"
        );
        assert_eq!(session.steps(), 0);
    }

    #[test]
    fn check_replies_with_the_check_and_refuses_a_schema_with_errors_showing_all_of_it() {
        let mut session = Session::default();
        let scratch = scratch("check");
        let schema = scratch.file("schema.csv", SCHEMA);
        let clean = scratch.file("clean.csv", "AssetId,Status,easting\nW-1,Live,1\n");
        let wrong = scratch.file("wrong.csv", "AssetId,Status,easting\nW-1,Alive,1\nW-2,,1\n");

        let passed = session.ok(&format!("UTILITY CHECK {clean} SCHEMA {schema}"));
        assert!(
            contains(
                &passed,
                "Delivery schema check against Example Utility Schema v0.1"
            ),
            "{passed}"
        );
        assert!(contains(
            &passed,
            "1 rows, 1 assets (AssetId): 0 errors, 0 warnings"
        ));

        let failed = session.refused(&format!("UTILITY CHECK {wrong} schema {schema}"));
        assert_eq!(failed.code, ErrorCode::InvalidArgument);
        assert!(
            contains(
                &failed.message,
                "the schedule does not meet the schema: 2 errors"
            ),
            "{}",
            failed.message
        );
        // The whole check, not a count: a script that stops says why.
        assert!(contains(
            &failed.message,
            "2 rows, 2 assets (AssetId): 2 errors, 0 warnings"
        ));
        assert!(
            contains(&failed.message, "\"Alive\" is not in the domain"),
            "{}",
            failed.message
        );
        assert!(contains(&failed.message, "Status is empty"));
        assert_eq!(session.steps(), 0);
    }

    // What `line` is refused with, described.
    #[track_caller]
    fn refusal(session: &mut Session, line: &str) -> String {
        session.refused(line).describe()
    }

    #[test]
    fn each_verb_refuses_what_it_cannot_run() {
        let mut session = Session::default();
        let scratch = scratch("refusals");
        let schedule = schedule();
        let design = design();
        let s = &mut session;
        assert!(contains(
            &refusal(s, "UTILITY"),
            "UTILITY REPORT | VERIFY | CLEARANCE | CHECK | DRAW"
        ));
        assert!(contains(
            &refusal(s, &format!("UTILITY PLOT {schedule}")),
            "UTILITY REPORT | VERIFY"
        ));
        assert!(contains(
            &refusal(s, "UTILITY REPORT"),
            "usage: UTILITY REPORT <schedule.csv>"
        ));
        assert!(contains(
            &refusal(s, &format!("UTILITY REPORT {schedule} MINCOVR 0.6")),
            "unknown option MINCOVR"
        ));
        assert!(contains(
            &refusal(
                s,
                &format!("UTILITY REPORT {schedule} MINCOVER 0.6 MINCOVER 1")
            ),
            "MINCOVER given twice"
        ));
        assert!(contains(
            &refusal(s, &format!("UTILITY REPORT {schedule} SPACING -1")),
            "SPACING needs a number of metres, not negative"
        ));
        assert!(contains(
            &refusal(s, &format!("UTILITY REPORT {schedule} MINCOVER")),
            "MINCOVER needs a number of metres"
        ));
        assert!(contains(
            &refusal(s, &format!("UTILITY VERIFY {schedule} extra")),
            "usage: UTILITY VERIFY <schedule.csv>"
        ));
        assert!(contains(
            &refusal(s, &format!("UTILITY CLEARANCE {schedule}")),
            "usage: UTILITY CLEARANCE"
        ));
        assert!(contains(
            &refusal(s, &format!("UTILITY CLEARANCE {schedule} {design} DEPTH 1")),
            "unknown option DEPTH"
        ));
        assert!(contains(
            &refusal(s, &format!("UTILITY CHECK {schedule} AGAINST {schedule}")),
            "usage: UTILITY CHECK <schedule.csv> SCHEMA <schema.csv>"
        ));
        assert!(contains(
            &refusal(s, "UTILITY DRAW"),
            "usage: UTILITY DRAW <schedule.csv>"
        ));
        assert!(contains(
            &refusal(s, &format!("UTILITY DRAW {schedule} LAYER")),
            "LAYER needs a layer name"
        ));
        assert!(contains(
            &refusal(s, &format!("UTILITY DRAW {schedule} WIDTH 1")),
            "unknown option WIDTH"
        ));
        assert!(contains(
            &refusal(s, &format!("UTILITY DRAW {schedule} LAYER a//b")),
            "not a layer path"
        ));
        // A prefix that is a path, but too deep for the two levels under it,
        // is refused as the prefix, before anything is made.
        assert!(contains(
            &refusal(
                s,
                &format!("UTILITY DRAW {schedule} LAYER a/b/c/d/e/f/g/h/i/j/k/l/m/n/o")
            ),
            "the layer prefix leaves no room"
        ));

        // A file that is not there is NotFound, by its path.
        let missing = session.refused(&format!(
            "UTILITY VERIFY \"{SAMPLES}/no such schedule.csv\""
        ));
        assert_eq!(missing.code, ErrorCode::NotFound);
        assert!(contains(&missing.message, "no such schedule.csv"));
        // A schedule that does not read says where.
        let broken = scratch.file(
            "broken.csv",
            "line,point,easting,northing,method,survace\nW1,W1-1,0,0,EML,20\n",
        );
        let unread = session.refused(&format!("UTILITY REPORT {broken}"));
        assert_eq!(unread.code, ErrorCode::ParseFailure);
        assert!(
            contains(&unread.describe(), "broken.csv"),
            "{}",
            unread.describe()
        );
        // A quote never closed is the tokenizer's refusal, as for every verb.
        assert!(contains(
            &session
                .refused(&format!("UTILITY REPORT \"{SAMPLES}/schedule.csv"))
                .describe(),
            "unterminated quoted string"
        ));

        assert_eq!(session.steps(), 0);
        assert_eq!(entities(&session), 0);
    }

    #[test]
    fn a_path_that_is_not_utf8_is_opened_as_the_narrow_name_it_is() {
        // katana_cli on Windows gets its arguments, its console and a script
        // saved from an ANSI editor in the ANSI code page, where the e-acute
        // of "cafe" is the one byte E9 - not UTF-8; the C++ case types that
        // byte. A line is a &str here, which holds no such byte (docs/rust.md,
        // step 2), so the narrow name cannot be typed. The UTF-8 half: a path
        // with that letter as UTF-8 is the name it is - not there, NotFound;
        // made, found and drawn.
        let mut session = Session::default();
        let scratch = scratch("narrow");
        let narrow = format!("{}/caf\u{E9}.csv", generic(&scratch.path));
        let missing = session.refused(&format!("UTILITY VERIFY \"{narrow}\""));
        assert_eq!(missing.code, ErrorCode::NotFound, "{}", missing.describe());

        if let Err(error) = std::fs::copy(format!("{SAMPLES}/schedule.csv"), &narrow) {
            eprintln!("this system makes no file of that name: {error}");
            return;
        }
        let reply = session.ok(&format!("UTILITY DRAW \"{narrow}\""));
        assert!(
            contains(&reply, "utilities drawn lines=4 vertices=14"),
            "{reply}"
        );
    }

    #[test]
    fn the_draw_replies_with_its_records_bounds_first() {
        let mut session = Session::default();
        let reply = session.ok(&format!("UTILITY DRAW {}", schedule()));
        assert_eq!(
            first_line(&reply),
            "utilities drawn lines=4 vertices=14 segments=10 entities=21 layers=11 \
             bounds=334000.000,6250000.000,334040.000,6250007.200"
        );
        assert!(
            contains(
                &reply,
                "\nline id=W1 type=water length=30.024 ql_a=1.420 ql_b=16.102 ql_c=12.502 \
                 ql_d=0.000"
            ),
            "{reply}"
        );
        assert!(contains(
            &reply,
            "\nline id=G1 type=gas length=40.000 ql_a=0.000 ql_b=0.000 ql_c=0.000 ql_d=40.000"
        ));
        assert_eq!(reply.matches('\n').count(), 4);
        assert_eq!(entities(&session), 21);

        // What a front end frames, read from the reply as it reads any other.
        let Some(bounds) = draw_reply_bounds(&reply) else {
            panic!("no bounds in {reply}");
        };
        double_eq(bounds.min.x, 334000.0);
        double_eq(bounds.min.y, 6250000.0);
        double_eq(bounds.max.x, 334040.0);
        double_eq(bounds.max.y, 6250007.2);
        // Numbers written with as few digits as they need, and a record read
        // back with a Windows line end, read as well.
        let Some(terse) = draw_reply_bounds("utilities drawn lines=1 bounds=1,2,3,4\r\n") else {
            panic!("no bounds in the terse record");
        };
        double_eq(terse.min.x, 1.0);
        double_eq(terse.max.y, 4.0);
        // Anything else frames nothing: another verb's reply, a bounds= that
        // is not the draw record's, or a box that is not four numbers, min
        // first.
        assert!(
            draw_reply_bounds(&session.ok(&format!("UTILITY VERIFY {}", schedule()))).is_none()
        );
        for other in [
            "AS 5488 subsurface utility investigation: 4 lines, 14 vertices\nbounds=1,2,3,4",
            "utilities drawn lines=1 bounds=1,2,3",
            "utilities drawn lines=1 bounds=1,2,3,4,5",
            "utilities drawn lines=1 bounds=1,2,x,4",
            "utilities drawn lines=1 bounds=3,2,1,4",
            "utilities drawn lines=1 bounds=1,4,3,2",
            "utilities drawn lines=1",
            "line id=W1 bounds=1,2,3,4",
            "",
        ] {
            assert!(draw_reply_bounds(other).is_none(), "{other}");
        }
    }

    #[test]
    fn a_drawn_schedule_is_one_undo_step() {
        let mut session = Session::default();
        let before = layers(&session);
        session.ok(&format!("UTILITY DRAW {}", schedule()));
        assert_eq!(session.steps(), 1);
        assert_eq!(session.document.history().undo_name(), "UTILITY DRAW");
        assert_eq!(entities(&session), 21);
        assert_eq!(layers(&session).len(), before.len() + 16);
        let qlb = layer(&session, "utilities/water/QL-B");
        assert_eq!(qlb.linetype, "utility-ql-b");
        assert_eq!(qlb.color, utility_type_colour(UtilityType::Water));
        for name in ["utility-ql-b", "utility-ql-c", "utility-ql-d"] {
            assert!(has_linetype(&session, name), "{name}");
        }

        session.ok("UNDO");
        assert_eq!(entities(&session), 0);
        assert_eq!(layers(&session), before);
        for name in ["utility-ql-b", "utility-ql-c", "utility-ql-d"] {
            assert!(!has_linetype(&session, name), "{name}");
        }

        session.ok("REDO");
        assert_eq!(entities(&session), 21);
        assert_eq!(layers(&session).len(), before.len() + 16);
        assert!(has_linetype(&session, "utility-ql-c"));
    }

    #[test]
    fn a_draw_reuses_a_layer_that_exists_and_leaves_it_as_it_was() {
        let mut session = Session::default();
        session.ok("LAYER NEW utilities/water/QL-B #123456");
        let mine = layer(&session, "utilities/water/QL-B");
        let layers_before = layers(&session).len();
        session.ok(&format!("UTILITY DRAW {}", schedule()));
        assert_eq!(session.steps(), 2);
        let after = layer(&session, "utilities/water/QL-B");
        assert_eq!(after, mine);
        assert_eq!(after.linetype, "continuous");
        // utilities, utilities/water and QL-B were there; 13 more are made.
        assert_eq!(layers(&session).len(), layers_before + 13);
        let mut on_mine = 0;
        session.document.model().entities.for_each(|entity| {
            on_mine += usize::from(entity.layer == "utilities/water/QL-B");
        });
        assert_eq!(on_mine, 1);
        // Undo takes the draw back and leaves the person's layer.
        session.ok("UNDO");
        assert_eq!(layers(&session).len(), layers_before);
        assert_eq!(layer(&session, "utilities/water/QL-B"), mine);

        // A second draw over the first reuses every layer and linetype.
        session.ok("REDO");
        let drawn_once = layers(&session).len();
        session.ok(&format!("UTILITY DRAW {}", schedule()));
        assert_eq!(layers(&session).len(), drawn_once);
        assert_eq!(entities(&session), 42);
        assert_eq!(session.steps(), 3);
    }

    #[test]
    fn a_refused_draw_changes_nothing() {
        let mut session = Session::default();
        let scratch = scratch("refused-draw");
        let one_vertex = scratch.file(
            "one vertex.csv",
            "line,point,easting,northing,method,h_unc,type\n\
             W1,W1-1,0,0,EML,0.1,water\n\
             W1,W1-2,10,0,EML,0.1,\n\
             W9,W9-1,5,5,EML,0.1,water\n",
        );
        let before = layers(&session);
        let error = session.refused(&format!("UTILITY DRAW {one_vertex}"));
        assert!(
            contains(&error.message, "line W9 cannot be graded"),
            "{}",
            error.describe()
        );
        assert!(contains(&error.message, "nothing was drawn"));
        assert_eq!(session.steps(), 0);
        assert_eq!(entities(&session), 0);
        assert_eq!(layers(&session), before);
        assert!(!has_linetype(&session, "utility-ql-b"));

        // A locked layer the draw would use refuses it when it runs, and the
        // layers and linetypes made before it are taken back.
        session.ok("LAYER NEW utilities/water/QL-A");
        session.ok("LAYER LOCK utilities/water/QL-A");
        let locked = layers(&session);
        let refused = session.refused(&format!("UTILITY DRAW {}", schedule()));
        assert!(
            contains(&refused.describe(), "locked"),
            "{}",
            refused.describe()
        );
        assert_eq!(session.steps(), 2);
        assert_eq!(entities(&session), 0);
        assert_eq!(layers(&session), locked);
        assert!(!has_linetype(&session, "utility-ql-b"));
    }

    #[test]
    fn records_a_rounding_error_apart_draw_as_the_report_grades_them() {
        // A pothole recorded twice, 10 nm apart, as a reprojected export
        // leaves it. REPORT grades the 0.000 m between them; DRAW refused the
        // whole schedule over it, with the model's "polyline has zero length".
        let mut session = Session::default();
        let scratch = scratch("near");
        let near = scratch.file(
            "near.csv",
            "line,point,easting,northing,method,level,level_ref,surface,h_unc,v_unc,path,type\n\
             S1,S1-1,334100.0,6250200.0,EML,,,,0.10,,,sewer\n\
             S1,S1-2,334105.0,6250200.0,pothole,19.0,top,20.0,0.02,0.02,exposed,\n\
             S1,S1-3,334105.00000001,6250200.0,pothole,19.0,top,20.0,0.02,0.02,,\n\
             S1,S1-4,334110.0,6250200.0,EML,,,,0.10,,,\n",
        );
        assert!(contains(
            &session.ok(&format!("UTILITY REPORT {near}")),
            "S1-2 -> S1-3  0.000 m  QL-A"
        ));
        let reply = session.ok(&format!("UTILITY DRAW {near}"));
        assert_eq!(
            first_line(&reply),
            "utilities drawn lines=1 vertices=4 segments=3 entities=6 layers=2 \
             bounds=334100.000,6250200.000,334110.000,6250200.000"
        );
        assert_eq!(session.steps(), 1);
        assert_eq!(entities(&session), 6);
    }

    #[test]
    fn layer_puts_the_drawing_under_another_prefix() {
        let mut session = Session::default();
        session.ok(&format!(
            "UTILITY DRAW {} LAYER \"Site Services/Located\"",
            schedule()
        ));
        let layers = &session.document.model().layers;
        assert!(layers.contains("Site Services"));
        assert!(layers.contains("Site Services/Located/water/QL-A"));
        assert!(layers.contains("Site Services/Located/gas/points"));
        assert!(!layers.contains("utilities"));
        session.ok("UNDO");
        assert!(!session.document.model().layers.contains("Site Services"));
    }

    #[test]
    fn spacing_and_min_cover_reach_the_drawing() {
        let mut session = Session::default();
        let reply = session.ok(&format!(
            "UTILITY DRAW {} SPACING 20 MINCOVER 0.85",
            schedule()
        ));
        assert!(
            contains(
                &reply,
                "line id=W1 type=water length=30.024 ql_a=1.420 ql_b=28.604 ql_c=0.000 \
                 ql_d=0.000"
            ),
            "{reply}"
        );
        assert!(
            !session
                .document
                .model()
                .layers
                .contains("utilities/water/QL-C")
        );
        let mut below = 0;
        session.document.model().entities.for_each(|entity| {
            // The C++ std::get<bool> throws for a value that is not one.
            match entity.properties.get(keys::COVER_BELOW_MINIMUM) {
                Some(PropertyValue::Boolean(true)) => below += 1,
                Some(PropertyValue::Boolean(false)) | None => {}
                Some(other) => panic!("{} is not a boolean: {other:?}", keys::COVER_BELOW_MINIMUM),
            }
        });
        // E1-1 0.800, E1-2 0.800, E1-4 0.780 below 0.85; E1-3 0.900 and W1's
        // are not.
        assert_eq!(below, 3);
    }

    #[test]
    fn help_utility_lists_every_option() {
        let mut session = Session::default();
        let help = session.ok("HELP UTILITY");
        for word in [
            "UTILITY REPORT",
            "UTILITY VERIFY",
            "UTILITY CLEARANCE",
            "UTILITY CHECK",
            "UTILITY DRAW",
            "MINCOVER",
            "SPACING",
            "WIDTH",
            "[H <m>]",
            "[V <m>]",
            "MARGIN",
            "SCHEMA",
            "LAYER <prefix>",
            "bounds=",
            "UTILITY DRAW <scope>",
            "[TYPE <type>]",
            "[METHOD <method>]",
            "[H_UNC <m>]",
            "[V_UNC <m>]",
            "[HEIGHTS surface|service|none]",
            "[LEVEL_REF top|centre|invert]",
            "[PATH detected|exposed|assumed]",
            "[OWNER <text>]",
            "[MATERIAL <text>]",
            "[DIAMETER_MM <mm>]",
            "[STATUS <status>]",
            "utility.heights",
            "points= loose= drawn= ignored=",
            "[FIELDS column=property,...]",
        ] {
            assert!(contains(&help, word), "{word}");
        }
        assert_eq!(help, utility_verb_help());
        assert!(contains(&CommandInterpreter::help_text(), "HELP UTILITY"));
    }
}
