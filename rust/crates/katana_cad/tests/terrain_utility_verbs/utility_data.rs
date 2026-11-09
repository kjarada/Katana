// The UTILITY verbs on what is drawn (utilities/utility_data.rs,
// scope_verbs.rs; tests/cad/utilities/test_utility_data.cpp): every verb but
// DRAW takes the services UTILITY DRAW drew, by the scope and filter words
// Global Modify takes, as well as a schedule file.
//
// The proofs the drawing must pass to stand for the schedule:
//   * a schedule drawn and then reported from the drawing gives the report
//     of the schedule, word for word;
//   * the schedule written from the drawing reads back as the lines drawn,
//     field for field and doubles to the bit;
//   * a point moved or a method edited, then REGRADE, draws exactly what a
//     DRAW of the schedule so edited draws - and one undo puts it back.
//
// The sample is samples/utilities, graded by hand in
// docs/subsurface_utilities.md.

mod utility_data {
    use katana_cad::utilities::utility_data::{
        DESIGN_CHORD_TOLERANCE, design_from_alignment, design_from_entity, design_stations,
        read_utility_data,
    };
    use katana_cad::utilities::utility_drawing::keys;
    use katana_cad::utilities::utility_verbs::{draw_reply_bounds, utility_verb_help};
    use katana_cad::{CommandInterpreter, Document, ScopeView};
    use katana_commands::create_alignment;
    use katana_core::text::format_exact_real;
    use katana_core::{ErrorCode, Result};
    use katana_entity::entity::{property_to_string, set_heights, to_string, type_name};
    use katana_entity::{Alignment, Color, Entity, EntityId, EntityType, Geometry, PropertyValue};
    use katana_geometry::alignment::{kind_of, solve_alignment};
    use katana_geometry::profile::solve_profile;
    use katana_geometry::{
        AlignmentElementKind, AlignmentPi, Circle2, HorizontalAlignment, Point2, Polyline2,
        ProfileElementKind, ProfilePvi, Segment2, VerticalAlignment,
    };
    use katana_survey::subsurface::{
        GradingSettings, parse_design_csv, render_investigation_report,
    };

    use crate::{
        SAMPLES, ScratchFolder, Session, contains, double_eq, first_line, must, near, ok, parsed,
        snapshot, spaced_search,
    };

    fn sample(name: &str) -> String {
        format!("\"{SAMPLES}/{name}\"")
    }

    #[track_caller]
    fn sample_text(name: &str) -> String {
        let path = format!("{SAMPLES}/{name}");
        match std::fs::read(&path) {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(error) => panic!("{path}: {error}"),
        }
    }

    fn after_first_line(text: &str) -> String {
        match text.find('\n') {
            Some(end) => String::from(&text[end + 1..]),
            None => String::new(),
        }
    }

    fn without_trailing_breaks(text: &str) -> String {
        String::from(text.trim_end_matches('\n'))
    }

    fn scratch(name: &str) -> ScratchFolder {
        ScratchFolder::new(&format!("katana-utility-data-{name}"))
    }

    // The drawn point of vertex `vertex`.
    #[track_caller]
    fn point(session: &Session, vertex: &str) -> EntityId {
        let mut found = 0;
        session.document.model().entities.for_each(|entity| {
            if entity.r#type() == EntityType::Point
                && entity
                    .properties
                    .get(keys::VERTEX)
                    .is_some_and(|id| property_to_string(id) == vertex)
            {
                found = entity.id;
            }
        });
        assert_ne!(found, 0, "no point {vertex}");
        found
    }

    fn select(session: &mut Session, ids: Vec<EntityId>) {
        session.document.selection_mut().set(ids);
        session.document.notify_selection_changed();
    }

    // An entity as a value that does not depend on its id: what two drawings
    // of one schedule must agree on, entity for entity.
    fn describe(entity: &Entity) -> String {
        let mut text = format!("{}|{}", entity.layer, to_string(entity.r#type()));
        match &entity.geometry {
            Geometry::Point(point) => {
                text += &format!(
                    " {},{}",
                    format_exact_real(point.position.x),
                    format_exact_real(point.position.y)
                );
            }
            Geometry::Polyline(line) => {
                for vertex in &line.vertices {
                    text += &format!(
                        " {},{}",
                        format_exact_real(vertex.x),
                        format_exact_real(vertex.y)
                    );
                }
            }
            _ => {}
        }
        text += &format!(
            "|{}|{}{}",
            entity.style,
            entity
                .color
                .map_or_else(|| String::from("ByLayer"), Color::to_hex),
            if entity.visible { "" } else { "|hidden" }
        );
        for (key, value) in &entity.properties {
            text += &format!("|{key}={}:{}", type_name(value), property_to_string(value));
        }
        text
    }

    fn drawing(document: &Document) -> Vec<String> {
        let mut entities = Vec::new();
        document
            .model()
            .entities
            .for_each(|entity| entities.push(describe(entity)));
        entities.sort();
        entities
    }

    // A schedule of the test's own drawn into a fresh document.
    fn drawn_from(quoted_path: &str, options: &str) -> Vec<String> {
        let mut fresh = Session::default();
        fresh.ok(&format!("UTILITY DRAW {quoted_path}{options}"));
        drawing(&fresh.document)
    }

    #[track_caller]
    fn replaced(text: &str, from: &str, to: &str) -> String {
        assert!(text.contains(from), "{from}");
        text.replacen(from, to, 1)
    }

    // A schema of the tests' own in the TfNSW Utility Schema's attribute
    // names, as tools/utility_schema_domains.py writes one: the real schema is
    // the client's document and stays out of the repository.
    fn schema(with_radar: bool) -> String {
        let mut text = String::from(
            "kind,attribute,value,detail,label\n\
             schema,Example Utility Schema,0.2,,\n\
             identifier,AssetIdentifier,,,\n\
             field,AssetIdentifier,Alphanumerical,Yes,Asset Identifier\n\
             field,AssetStatus,Domain List: Asset Status,Yes,Asset Status\n\
             domain,AssetStatus,In Service,,\n\
             domain,AssetStatus,Abandoned,,\n\
             field,LocateMethod,Domain List: Locate Method,Yes,Locate Method\n\
             domain,LocateMethod,Electronic Detection,,\n\
             domain,LocateMethod,Potholing,,\n",
        );
        if with_radar {
            text += "domain,LocateMethod,Ground Penetrating Radar,,\n";
        }
        text += "domain,LocateMethod,Archive Drawings and Plans,,\n\
                 domain,LocateMethod,Geographic Information System,,\n\
                 field,QualityLevel,Domain List: Quality Level,Yes,Quality Level\n\
                 domain,QualityLevel,Quality Level A,,\n\
                 domain,QualityLevel,Quality Level B,,\n\
                 domain,QualityLevel,Quality Level C,,\n\
                 domain,QualityLevel,Quality Level D,,\n";
        text
    }

    fn pi_at(x: f64, y: f64, radius: f64) -> AlignmentPi {
        AlignmentPi {
            point: Point2::new(x, y),
            radius,
            ..AlignmentPi::default()
        }
    }

    fn pvi(station: f64, elevation: f64, curve_length: f64) -> ProfilePvi {
        ProfilePvi {
            station,
            elevation,
            curve_length,
        }
    }

    // ---- the drawing is the schedule ------------------------------------------------------

    #[test]
    fn the_report_of_the_drawing_is_the_report_of_the_schedule_it_was_drawn_from() {
        for name in ["schedule.csv", "schedule_tfnsw.csv"] {
            for options in [" MINCOVER 0.6", " SPACING 20", ""] {
                let mut session = Session::default();
                let from_file = session.ok(&format!("UTILITY REPORT {}{options}", sample(name)));
                session.ok(&format!("UTILITY DRAW {}", sample(name)));
                let from_drawing = session.ok(&format!("UTILITY REPORT DRAWING{options}"));
                assert_eq!(
                    after_first_line(&from_drawing),
                    from_file,
                    "{name}{options}"
                );
            }
        }
        let mut session = Session::default();
        session.ok(&format!("UTILITY DRAW {}", sample("schedule.csv")));
        // 7 runs and 14 points, 4 lines, all of each.
        assert_eq!(
            first_line(&session.ok("UTILITY REPORT DRAWING")),
            "scope=drawing matched=21 lines=4 completed=0 ignored=0"
        );
    }

    #[test]
    fn verify_clearance_and_check_of_the_drawing_are_those_of_the_schedule() {
        let mut session = Session::default();
        let schedule = sample("schedule.csv");
        let design = sample("design.csv");
        session.ok(&format!("UTILITY DRAW {schedule}"));
        let verified = after_first_line(&session.ok("UTILITY VERIFY DRAWING"));
        assert_eq!(verified, session.ok(&format!("UTILITY VERIFY {schedule}")));
        let from_file = session.ok(&format!(
            "UTILITY CLEARANCE {schedule} {design} WIDTH 0.375"
        ));
        assert_eq!(
            after_first_line(&session.ok(&format!(
                "UTILITY CLEARANCE DRAWING DESIGN {design} WIDTH 0.375"
            ))),
            from_file
        );
        // DESIGN may be left out, as the file's second place always took it.
        assert_eq!(
            after_first_line(
                &session.ok(&format!("UTILITY CLEARANCE DRAWING {design} WIDTH 0.375"))
            ),
            from_file
        );
        assert_eq!(
            session.ok(&format!(
                "UTILITY CLEARANCE {schedule} DESIGN {design} WIDTH 0.375"
            )),
            from_file
        );
        // None of them touched the drawing.
        assert_eq!(session.steps(), 1);
    }

    #[test]
    fn the_drawing_read_back_is_the_schedule_it_was_drawn_from() {
        for name in ["schedule.csv", "schedule_tfnsw.csv"] {
            let mut session = Session::default();
            session.ok(&format!("UTILITY DRAW {}", sample(name)));
            let mut everything: Vec<EntityId> = Vec::new();
            session
                .document
                .model()
                .entities
                .for_each(|entity| everything.push(entity.id));
            let data = must(read_utility_data(session.document.model(), &everything));
            assert_eq!(data.lines(), parsed(&sample_text(name)), "{name}");
            assert_eq!(data.ignored, 0);
            assert_eq!(data.completed, 0);
        }
    }

    #[test]
    fn a_schedule_written_from_the_drawing_reads_back_as_the_lines_drawn() {
        let scratch = scratch("schedule");
        for name in ["schedule.csv", "schedule_tfnsw.csv"] {
            let mut session = Session::default();
            session.ok(&format!("UTILITY DRAW {}", sample(name)));
            let out = scratch.at(&format!("out {name}"));
            let reply = session.ok(&format!("UTILITY SCHEDULE \"{out}\" DRAWING"));
            let vertices = if name == "schedule.csv" { 14 } else { 7 };
            let lines = if name == "schedule.csv" { 4 } else { 3 };
            assert_eq!(
                first_line(&reply),
                format!("utilities scheduled path=\"{out}\" lines={lines} vertices={vertices}")
            );
            assert!(
                after_first_line(&reply).starts_with("scope=drawing matched="),
                "{reply}"
            );
            assert_eq!(
                parsed(&scratch.read(&format!("out {name}"))),
                parsed(&sample_text(name)),
                "{name}"
            );
            // The report of the schedule written is the report of the one
            // drawn.
            let written = session.ok(&format!("UTILITY REPORT \"{out}\" MINCOVER 0.6"));
            assert_eq!(
                written,
                session.ok(&format!("UTILITY REPORT {} MINCOVER 0.6", sample(name)))
            );
            assert_eq!(session.steps(), 1);
        }
    }

    // ---- what the scope takes -------------------------------------------------------------

    #[test]
    fn a_part_of_a_line_takes_the_whole_line_and_the_reply_says_so() {
        let mut session = Session::default();
        session.ok(&format!("UTILITY DRAW {}", sample("schedule.csv")));
        let lines = parsed(&sample_text("schedule.csv"));
        let water_only = without_trailing_breaks(&must(render_investigation_report(
            std::slice::from_ref(&lines[0]),
            &GradingSettings::default(),
            None,
        )));

        // One point of W1 selected: all of W1 is reported.
        let w1_3 = point(&session, "W1-3");
        select(&mut session, vec![w1_3]);
        let reply = session.ok("UTILITY REPORT SELECTION");
        assert_eq!(
            first_line(&reply),
            "scope=selection matched=1 lines=1 completed=1 ignored=0"
        );
        assert_eq!(after_first_line(&reply), water_only);

        // Only W1's QL-B run: W1 whole again, from its points.
        let reply = session.ok("UTILITY REPORT LAYERS utilities/water/QL-B");
        assert_eq!(
            first_line(&reply),
            "scope=layers layers=utilities/water/QL-B sublayers=yes matched=1 lines=1 \
             completed=1 ignored=0"
        );
        assert_eq!(after_first_line(&reply), water_only);

        // Every layer of water: its three runs and six points, nothing
        // completed.
        let reply = session.ok("UTILITY REPORT LAYERS utilities/water");
        assert_eq!(
            first_line(&reply),
            "scope=layers layers=utilities/water sublayers=yes matched=9 lines=1 completed=0 \
             ignored=0"
        );

        // The filter narrows as MODIFY's does: the gas main's run and points.
        let reply = session.ok("UTILITY REPORT DRAWING WHERE PROP=utility.type:gas");
        assert_eq!(
            first_line(&reply),
            "scope=drawing where=\"PROP=utility.type:gas\" matched=3 lines=1 completed=0 \
             ignored=0"
        );
        assert!(contains(&reply, "G1"), "{reply}");
        assert!(!contains(&reply, "W1-3"), "{reply}");

        // A window over the gas main's points (y 6250007 to 6250007.2) takes
        // its points and its run, and not the telecommunications run at y
        // 6250005.5.
        let reply = session.ok("UTILITY REPORT AREA 333990,6250006.5,334050,6250008");
        assert_eq!(
            first_line(&reply),
            "scope=area area=333990,6250006.5,334050,6250008 matched=3 lines=1 completed=0 \
             ignored=0"
        );
    }

    #[test]
    fn what_carries_no_utility_data_is_ignored_and_counted() {
        let mut session = Session::default();
        session.ok(&format!("UTILITY DRAW {}", sample("schedule.csv")));
        session.ok("LINE 0,0 10,10");
        session.ok("POINT 5,5");
        let reply = session.ok("UTILITY REPORT DRAWING MINCOVER 0.6");
        assert_eq!(
            first_line(&reply),
            "scope=drawing matched=23 lines=4 completed=0 ignored=2"
        );
        assert_eq!(
            after_first_line(&reply),
            session.ok(&format!(
                "UTILITY REPORT {} MINCOVER 0.6",
                sample("schedule.csv")
            ))
        );
    }

    #[test]
    fn a_scope_that_takes_nothing_says_so_and_is_no_failure() {
        let mut session = Session::default();
        session.ok(&format!("UTILITY DRAW {}", sample("schedule.csv")));
        let scratch = scratch("nothing");
        let schema_file = scratch.file("schema.csv", &schema(true));
        let record = "scope=selection matched=0 lines=0 completed=0 ignored=0\n\
                      no utility lines in the scope: nothing ";
        assert_eq!(
            session.ok("UTILITY REPORT SELECTION"),
            format!("{record}to report")
        );
        assert_eq!(
            session.ok("UTILITY VERIFY SELECTION"),
            format!("{record}to verify")
        );
        assert_eq!(
            session.ok(&format!(
                "UTILITY CLEARANCE SELECTION DESIGN {}",
                sample("design.csv")
            )),
            format!("{record}to clear")
        );
        assert_eq!(
            session.ok(&format!("UTILITY CHECK SELECTION SCHEMA {schema_file}")),
            format!("{record}to check")
        );
        assert_eq!(
            session.ok("UTILITY REGRADE SELECTION"),
            format!("{record}to regrade")
        );
        assert_eq!(
            session.ok(&format!(
                "UTILITY SCHEDULE \"{}\" SELECTION",
                scratch.at("none.csv")
            )),
            format!("{record}written")
        );
        assert!(!scratch.path.join("none.csv").exists());
        assert_eq!(session.steps(), 1);
    }

    #[test]
    fn the_windows_view_is_a_scope_and_headless_it_is_refused_naming_area() {
        let mut session = Session::default();
        session.ok(&format!("UTILITY DRAW {}", sample("schedule.csv")));
        let headless = session.refused("UTILITY REPORT VIEW");
        assert_eq!(headless.code, ErrorCode::InvalidState);
        assert!(
            contains(&headless.message, "AREA x0,y0,x1,y1"),
            "{}",
            headless.message
        );

        // A plan view hiding the gas services draws the other three.
        session.interpreter.set_scope_context(Some(Box::new(
            |_: Option<u32>| -> Result<ScopeView> {
                let mut view = ScopeView {
                    id: 2,
                    ..ScopeView::default()
                };
                view.layers.hide("utilities/gas");
                Ok(view)
            },
        )));
        let reply = session.ok("UTILITY REPORT VIEW");
        assert_eq!(
            first_line(&reply),
            "scope=view view=2 matched=18 lines=3 completed=0 ignored=0"
        );
    }

    #[test]
    fn a_path_typed_on_a_point_reads_as_the_schedules_path_column_reads_it() {
        // The schedule's path column takes the field words "traced", "trench"
        // and "inferred" too (subsurface::parse_path_evidence); the drawing
        // took only the three it writes, and refused "trench" typed in the
        // property panel for the pothole's open trench, which the schedule
        // would read.
        let mut session = Session::default();
        session.ok(&format!("UTILITY DRAW {}", sample("schedule.csv")));
        let report = session.ok("UTILITY REPORT DRAWING");
        let crossing = point(&session, "W1-X1");
        select(&mut session, vec![crossing]);
        session.ok("PROP SET utility.path trench text");
        assert_eq!(session.ok("UTILITY REPORT DRAWING"), report);
        session.ok("PROP SET utility.path Trench text");
        assert_eq!(session.ok("UTILITY REPORT DRAWING"), report);
        session.ok("PROP SET utility.path dug text");
        assert!(contains(
            &session.refused("UTILITY REPORT DRAWING").message,
            "has utility.path \"dug\", which is not detected, exposed or assumed"
        ));
    }

    #[test]
    fn points_that_do_not_agree_are_refused_by_name() {
        let mut session = Session::default();
        session.ok(&format!("UTILITY DRAW {}", sample("schedule.csv")));
        let first = point(&session, "W1-1");
        let second = point(&session, "W1-2");

        // Two points at one place along the line.
        select(&mut session, vec![second]);
        session.ok("PROP SET utility.order 1 integer");
        let why = session.refused("UTILITY REPORT DRAWING").message;
        assert_eq!(
            why,
            format!("line W1 has two points at utility.order 1: #{first} and #{second}")
        );
        session.ok("UNDO");

        // A point with no method.
        session.ok("PROP DELETE utility.method");
        let why = session.refused("UTILITY REPORT DRAWING").message;
        assert_eq!(
            why,
            format!(
                "line W1: point W1-2 (#{second}) has no utility.method, the method it was \
                 located by"
            )
        );
        session.ok("UNDO");

        // A point that says another owner than its line's first.
        session.ok("PROP SET utility.owner \"Other Co\"");
        let why = session.refused("UTILITY REPORT DRAWING").message;
        assert_eq!(
            why,
            format!(
                "line W1: point W1-2 (#{second}) has utility.owner \"Other Co\" where point W1-1 \
                 (#{first}) has \"WaterCo\"; the points of one line must agree"
            )
        );
        session.ok("UNDO");

        // A level that is not a number.
        session.ok("PROP SET utility.level deep text");
        assert!(contains(
            &session.refused("UTILITY REPORT DRAWING").message,
            "has utility.level \"deep\", which is not a number"
        ));
        session.ok("UNDO");

        // A method nobody knows.
        session.ok("PROP SET utility.method dowsing");
        assert!(contains(
            &session
                .refused("UTILITY REPORT LAYERS utilities/water")
                .message,
            "utility.method \"dowsing\", which is not a location method"
        ));
        session.ok("UNDO");

        // A line whose points were all erased, its run left.
        let g1_1 = point(&session, "G1-1");
        let g1_2 = point(&session, "G1-2");
        select(&mut session, vec![g1_1, g1_2]);
        session.ok("ERASE");
        assert!(contains(
            &session
                .refused("UTILITY REPORT LAYERS utilities/gas")
                .message,
            "line G1 has no points left to read it from"
        ));
        session.ok("UNDO");

        // A schedule drawn twice is two points at every place of every line.
        session.ok(&format!("UTILITY DRAW {}", sample("schedule.csv")));
        assert!(contains(
            &session.refused("UTILITY REPORT DRAWING").message,
            "has two points at utility.order 1"
        ));

        // A person's own entities outside a scope are not read: a scope that
        // takes only what agrees is answered.
        session.ok("UNDO");
        assert!(
            session
                .ok("UTILITY REPORT DRAWING")
                .starts_with("scope=drawing matched=21")
        );
    }

    #[test]
    fn a_point_whose_line_was_deleted_refuses_its_line_rather_than_leaving_it_short() {
        let mut session = Session::default();
        session.ok(&format!("UTILITY DRAW {}", sample("schedule.csv")));
        let lost = point(&session, "W1-2");
        select(&mut session, vec![lost]);
        session.ok("PROP DELETE utility.line");
        let why = format!(
            "point W1-2 (#{lost}) is a vertex of a schedule and has no utility.line, the line it \
             is a point of; by its attributes it is a point of line W1; give it its line again, \
             or delete its utility.vertex and utility.order"
        );
        // In the scope: not "ignored", as a point of no schedule is.
        assert_eq!(session.refused("UTILITY REPORT DRAWING").message, why);
        // Outside it: W1, read whole from its other points, would be W1 short
        // of a vertex - graded, redrawn and written so.
        let w1_3 = point(&session, "W1-3");
        select(&mut session, vec![w1_3]);
        let steps = session.steps();
        let before = snapshot(&session.document);
        assert_eq!(session.refused("UTILITY REGRADE SELECTION").message, why);
        assert_eq!(session.steps(), steps);
        assert_eq!(snapshot(&session.document), before);
        let scratch = scratch("stray");
        assert_eq!(
            session
                .refused(&format!(
                    "UTILITY SCHEDULE \"{}\" LAYERS utilities/water",
                    scratch.at("out.csv")
                ))
                .message,
            why
        );
        assert!(!scratch.path.join("out.csv").exists());
        // A line it is not a point of reads as it did.
        assert_eq!(
            first_line(&session.ok("UTILITY REPORT LAYERS utilities/gas")),
            "scope=layers layers=utilities/gas sublayers=yes matched=3 lines=1 completed=0 \
             ignored=0"
        );
        // Given its line again, W1 reads whole.
        session.ok("UNDO");
        assert_eq!(
            first_line(&session.ok("UTILITY REPORT DRAWING")),
            "scope=drawing matched=21 lines=4 completed=0 ignored=0"
        );
    }

    // ---- REGRADE --------------------------------------------------------------------------

    #[test]
    fn regrade_after_a_moved_point_draws_what_a_draw_of_the_schedule_so_edited_draws() {
        let scratch = scratch("moved");
        let mut session = Session::default();
        session.ok(&format!("UTILITY DRAW {}", sample("schedule.csv")));
        // W1-5 3 m back towards the pothole: 9.502 m from W1-4, inside the 10
        // m detected spacing, so W1-4 -> W1-5 is QL-B now, not QL-C.
        let w1_5 = point(&session, "W1-5");
        select(&mut session, vec![w1_5]);
        session.ok("MOVE -3,0");
        let before = snapshot(&session.document);
        let before_layers = session.document.model().layers.names();
        let steps = session.steps();

        let reply = session.ok("UTILITY REGRADE DRAWING");
        assert_eq!(
            first_line(&reply),
            "utilities regraded changed=1 lines=4 vertices=14 segments=10 entities=21 layers=10 \
             bounds=334000.000,6250000.000,334040.000,6250007.200"
        );
        assert!(
            contains(
                &reply,
                "\nscope=drawing matched=21 lines=4 completed=0 ignored=0\n"
            ),
            "{reply}"
        );
        assert!(
            contains(
                &reply,
                "\nline id=W1 type=water length=27.024 ql_a=1.420 ql_b=25.604 ql_c=0.000 \
                 ql_d=0.000"
            ),
            "{reply}"
        );
        assert_eq!(session.steps(), steps + 1);
        assert_eq!(session.document.history().undo_name(), "UTILITY REGRADE");

        // Exactly what a DRAW of the schedule with W1-5 there draws.
        let edited = scratch.file(
            "moved.csv",
            &replaced(
                &sample_text("schedule.csv"),
                "W1,W1-5,334030.000,",
                "W1,W1-5,334027.000,",
            ),
        );
        assert_eq!(drawing(&session.document), drawn_from(&edited, ""));
        // The other services' entities are the same entities, untouched.
        let after = snapshot(&session.document);
        for (id, entity) in &before {
            let Some(line) = entity.properties.get(keys::LINE) else {
                panic!("#{id} has no {}", keys::LINE);
            };
            if property_to_string(line) != "W1" {
                assert!(after.contains_key(id));
                assert_eq!(after[id], *entity);
            } else if entity.r#type() == EntityType::Point {
                assert!(after.contains_key(id), "a point keeps its id");
            }
        }

        // One undo puts every entity back as it was, ids and all.
        session.ok("UNDO");
        assert_eq!(snapshot(&session.document), before);
        assert_eq!(session.document.model().layers.names(), before_layers);
        session.ok("REDO");
        assert_eq!(drawing(&session.document), drawn_from(&edited, ""));
    }

    #[test]
    fn regrade_after_an_edited_method_draws_what_a_draw_of_the_schedule_so_edited_draws() {
        let scratch = scratch("method");
        let mut session = Session::default();
        session.ok(&format!(
            "UTILITY DRAW {} MINCOVER 0.6",
            sample("schedule.csv")
        ));
        // W1-1 said now to be from the records: QL-D, and its segment with it.
        let w1_1 = point(&session, "W1-1");
        select(&mut session, vec![w1_1]);
        session.ok("PROP SET utility.method records");
        session.ok("UTILITY REGRADE LAYERS utilities/water MINCOVER 0.6");
        let edited = scratch.file(
            "method.csv",
            &replaced(
                &sample_text("schedule.csv"),
                "W1,W1-1,334000.000,6250000.000,EML,",
                "W1,W1-1,334000.000,6250000.000,records,",
            ),
        );
        assert_eq!(
            drawing(&session.document),
            drawn_from(&edited, " MINCOVER 0.6")
        );
        let regraded = point(&session, "W1-1");
        let Some(entity) = session.document.model().entities.find(regraded) else {
            panic!("no entity #{regraded}");
        };
        assert_eq!(
            property_to_string(&entity.properties[keys::QUALITY_LEVEL]),
            "QL-D"
        );
    }

    #[test]
    fn regrade_follows_an_edited_type_to_its_layers() {
        let scratch = scratch("type");
        let mut session = Session::default();
        session.ok(&format!("UTILITY DRAW {}", sample("schedule.csv")));
        session
            .ok("MODIFY DRAWING WHERE TYPE=point PROP=utility.line:T1 SET PROP=utility.type:sewer");
        session.ok("UTILITY REGRADE DRAWING WHERE PROP=utility.line:T1");
        let edited = scratch.file(
            "type.csv",
            &replaced(
                &sample_text("schedule.csv"),
                "assumed,,telecommunications,",
                "assumed,,sewer,",
            ),
        );
        assert_eq!(drawing(&session.document), drawn_from(&edited, ""));
        assert!(
            session
                .document
                .model()
                .layers
                .contains("utilities/sewer/QL-C")
        );
        assert!(
            session
                .document
                .model()
                .layers
                .contains("utilities/sewer/points")
        );
    }

    #[test]
    fn a_run_given_an_arc_is_still_its_lines_run_and_a_regrade_draws_it_again() {
        // A run with an arc is stored as a curve polyline (docs/drawing.md,
        // "Which kind a polyline is"). Still its line's run: were it not, a
        // regrade would draw the line's runs again beside it.
        let mut session = Session::default();
        session.ok(&format!("UTILITY DRAW {}", sample("schedule.csv")));
        let mut run: EntityId = 0;
        session.document.model().entities.for_each(|entity| {
            if run == 0
                && entity.r#type() == EntityType::Polyline
                && entity
                    .properties
                    .get(keys::LINE)
                    .is_some_and(|line| property_to_string(line) == "W1")
            {
                run = entity.id;
            }
        });
        assert_ne!(run, 0);
        session.ok(&format!("VERTEX SET {run} 0 bulge=0.1"));
        assert_eq!(
            session
                .document
                .model()
                .entities
                .find(run)
                .map(Entity::r#type),
            Some(EntityType::CurvePolyline)
        );

        session.ok("UTILITY REGRADE DRAWING");
        assert_eq!(
            drawing(&session.document),
            drawn_from(&sample("schedule.csv"), ""),
            "the arced run was left beside the runs drawn again"
        );
        assert!(session.document.model().entities.find(run).is_none());
    }

    #[test]
    fn regrade_of_what_nobody_edited_changes_nothing_and_pushes_no_step() {
        let mut session = Session::default();
        session.ok(&format!("UTILITY DRAW {}", sample("schedule.csv")));
        let before = snapshot(&session.document);
        let reply = session.ok("UTILITY REGRADE DRAWING");
        assert!(
            reply.starts_with(
                "utilities regraded changed=0 lines=4 vertices=14 segments=10 entities=21 \
                 layers=11 "
            ),
            "{reply}"
        );
        assert_eq!(session.steps(), 1);
        assert_eq!(snapshot(&session.document), before);
        // The bounds a front end frames are read from a regrade as from a
        // draw.
        let Some(bounds) = draw_reply_bounds(&reply) else {
            panic!("no bounds in {reply}");
        };
        double_eq(bounds.max.y, 6250007.2);
    }

    #[test]
    fn regrade_grades_each_line_with_the_settings_it_was_drawn_with_unless_told_otherwise() {
        // Unedited, whatever it was drawn with: nothing to regrade - no cover
        // flag stripped, no run graded at the default spacing instead.
        for options in [" MINCOVER 0.6", " SPACING 20", " SPACING 20 MINCOVER 0.6"] {
            let mut session = Session::default();
            session.ok(&format!("UTILITY DRAW {}{options}", sample("schedule.csv")));
            let before = snapshot(&session.document);
            let reply = session.ok("UTILITY REGRADE DRAWING");
            assert!(
                reply.starts_with("utilities regraded changed=0 "),
                "{options}\n{reply}"
            );
            assert_eq!(snapshot(&session.document), before, "{options}");
            assert_eq!(session.steps(), 1, "{options}");
        }

        // What the points say they were graded with.
        let scratch = scratch("settings");
        let mut session = Session::default();
        session.ok(&format!(
            "UTILITY DRAW {} SPACING 20",
            sample("schedule.csv")
        ));
        let w1_5 = point(&session, "W1-5");
        {
            let Some(drawn_point) = session.document.model().entities.find(w1_5) else {
                panic!("no entity #{w1_5}");
            };
            assert_eq!(
                property_to_string(&drawn_point.properties[keys::SPACING]),
                property_to_string(&PropertyValue::Real(20.0))
            );
            assert!(!drawn_point.properties.contains_key(keys::MINIMUM_COVER));
        }

        // One point moved: its line alone changes, graded at the 20 m it was
        // drawn with - what a DRAW of the schedule so edited at 20 m draws.
        select(&mut session, vec![w1_5]);
        session.ok("MOVE -3,0");
        assert!(
            session
                .ok("UTILITY REGRADE DRAWING")
                .starts_with("utilities regraded changed=1 ")
        );
        let edited = scratch.file(
            "moved.csv",
            &replaced(
                &sample_text("schedule.csv"),
                "W1,W1-5,334030.000,",
                "W1,W1-5,334027.000,",
            ),
        );
        assert_eq!(
            drawing(&session.document),
            drawn_from(&edited, " SPACING 20")
        );

        // Told otherwise, every line in scope takes what it is told - and
        // keeps it.
        session.ok("UTILITY REGRADE DRAWING SPACING 10");
        assert_eq!(
            drawing(&session.document),
            drawn_from(&edited, " SPACING 10")
        );
        assert!(
            session
                .ok("UTILITY REGRADE DRAWING")
                .starts_with("utilities regraded changed=0 ")
        );

        // Points of one line that disagree on them are refused, as on any
        // attribute of the line.
        let w1_2 = point(&session, "W1-2");
        select(&mut session, vec![w1_2]);
        session.ok("PROP SET utility.spacing 15 real");
        let why = session.refused("UTILITY REGRADE DRAWING").message;
        assert!(contains(&why, "has utility.spacing \"15"), "{why}");
    }

    #[test]
    fn regrade_keeps_what_is_the_persons_on_a_point() {
        let mut session = Session::default();
        session.ok(&format!("UTILITY DRAW {}", sample("schedule.csv")));
        let moved = point(&session, "W1-5");
        select(&mut session, vec![moved]);
        session.ok("PROP SET note \"check with the owner\"");
        session.ok("PROP SET utility.note mine");
        session.ok("COLOR #FF00FF");
        session.ok("MOVE -3,0");
        session.ok("UTILITY REGRADE SELECTION");
        let Some(kept) = session.document.model().entities.find(moved) else {
            panic!("no entity #{moved}");
        };
        assert_eq!(
            property_to_string(&kept.properties["note"]),
            "check with the owner"
        );
        assert_eq!(property_to_string(&kept.properties["utility.note"]), "mine");
        assert_eq!(kept.color, Some(Color::new(255, 0, 255, 255)));
        assert_eq!(kept.layer, "utilities/water/points");
    }

    #[test]
    fn regrade_is_one_step_and_all_or_nothing() {
        let mut session = Session::default();
        session.ok(&format!("UTILITY DRAW {}", sample("schedule.csv")));
        let w1_5 = point(&session, "W1-5");
        select(&mut session, vec![w1_5]);
        session.ok("MOVE -3,0");
        // The QL-C run the regrade must take away is on a locked layer.
        session.ok("LAYER LOCK utilities/water/QL-C");
        let before = snapshot(&session.document);
        let layers = session.document.model().layers.names();
        let steps = session.steps();
        let locked = session.refused("UTILITY REGRADE DRAWING");
        assert!(
            contains(&locked.describe(), "locked"),
            "{}",
            locked.describe()
        );
        assert_eq!(snapshot(&session.document), before);
        assert_eq!(session.document.model().layers.names(), layers);
        assert_eq!(session.steps(), steps);

        // A line that does not read refuses the whole regrade by its name.
        session.ok("LAYER UNLOCK utilities/water/QL-C");
        let e1_2 = point(&session, "E1-2");
        select(&mut session, vec![e1_2]);
        session.ok("PROP DELETE utility.order");
        let unlocked_steps = session.steps();
        let unread = session.refused("UTILITY REGRADE DRAWING");
        assert!(
            contains(&unread.message, "line E1: point E1-2"),
            "{}",
            unread.message
        );
        assert_eq!(session.steps(), unlocked_steps);
    }

    #[test]
    fn the_verbs_refuse_a_source_they_do_not_take() {
        let mut session = Session::default();
        let schedule = sample("schedule.csv");
        assert!(contains(
            &session
                .refused(&format!("UTILITY REGRADE {schedule}"))
                .describe(),
            "usage: UTILITY REGRADE <scope>"
        ));
        assert!(contains(
            &session.refused("UTILITY REGRADE").describe(),
            "usage: UTILITY REGRADE <scope>"
        ));
        assert!(contains(
            &session
                .refused(&format!("UTILITY SCHEDULE out.csv {schedule}"))
                .describe(),
            "usage: UTILITY SCHEDULE <out.csv> <scope>"
        ));
        assert!(contains(
            &session
                .refused("UTILITY SCHEDULE out.csv DRAWING SCHEMA")
                .describe(),
            "usage: UTILITY SCHEDULE"
        ));
        assert!(contains(
            &session
                .refused("UTILITY REPORT DRAWING WHERE SHADE=blue")
                .describe(),
            "not a WHERE key"
        ));
        assert!(contains(
            &session.refused("UTILITY REPORT LAYERS nowhere").describe(),
            "layer does not exist"
        ));
        assert!(contains(
            &session.refused("UTILITY VERIFY DRAWING extra").describe(),
            "usage: UTILITY VERIFY"
        ));
        assert!(contains(
            &session.refused("UTILITY CHECK DRAWING").describe(),
            "usage: UTILITY CHECK"
        ));
        assert_eq!(session.steps(), 0);
    }

    // ---- CHECK and SCHEDULE in a client's words -------------------------------------------

    #[test]
    fn the_drawing_is_checked_as_the_schedule_it_would_write_in_the_schemas_words() {
        let scratch = scratch("check");
        let with_radar = scratch.file("schema.csv", &schema(true));
        let without_radar = scratch.file("no radar.csv", &schema(false));
        let mut session = Session::default();
        // The file says E-0001 is "In service" on both its rows, which the
        // schema lists as "In Service": a warning each.
        let from_file = session.ok(&format!(
            "UTILITY CHECK {} SCHEMA {with_radar}",
            sample("schedule_tfnsw.csv")
        ));
        assert!(
            contains(
                &from_file,
                "7 rows, 3 assets (AssetIdentifier): 0 errors, 2 warnings"
            ),
            "{from_file}"
        );

        // The drawing holds the status, not a spelling of it, and writes it
        // as the schema lists it.
        session.ok(&format!("UTILITY DRAW {}", sample("schedule_tfnsw.csv")));
        let from_drawing = session.ok(&format!("UTILITY CHECK DRAWING SCHEMA {with_radar}"));
        // Seven points and a run each: W-0001's two detected stretches are
        // both QL-B, E-0001's 18 m is past the detected spacing, G-0001 is
        // records.
        assert_eq!(
            first_line(&from_drawing),
            "scope=drawing matched=10 lines=3 completed=0 ignored=0"
        );
        assert!(
            contains(
                &from_drawing,
                "7 rows, 3 assets (AssetIdentifier): 0 errors, 0 warnings"
            ),
            "{from_drawing}"
        );

        // A method the schema has no spelling for is written in Katana's own,
        // for the check to find, and the check refuses with all of itself.
        let refused = session.refused(&format!(
            "UTILITY CHECK LAYERS utilities/electricity SCHEMA {without_radar}"
        ));
        assert!(
            refused.message.starts_with(
                "the schedule does not meet the schema: 2 errors\nscope=layers \
                 layers=utilities/electricity"
            ),
            "{}",
            refused.message
        );
        assert!(
            contains(
                &refused.message,
                "\"ground penetrating radar\" is not in the domain"
            ),
            "{}",
            refused.message
        );

        // SCHEDULE with the schema writes that same deliverable, and it reads
        // back as the lines drawn.
        let out = scratch.at("deliverable.csv");
        session.ok(&format!(
            "UTILITY SCHEDULE \"{out}\" DRAWING SCHEMA {with_radar}"
        ));
        let written = scratch.read("deliverable.csv");
        let header = first_line(&written);
        for column in [
            "AssetIdentifier",
            "LocateMethod",
            "QualityLevel",
            "AssetStatus",
        ] {
            assert!(contains(&header, column), "{header}");
        }
        assert!(contains(&written, "In Service"), "{written}");
        assert!(contains(&written, "Electronic Detection"), "{written}");
        assert!(contains(&written, "Quality Level B"), "{written}");
        assert_eq!(parsed(&written), parsed(&sample_text("schedule_tfnsw.csv")));
    }

    #[test]
    fn what_the_schedule_said_it_did_not_know_is_kept_so_the_drawing_meets_the_same_schema() {
        // Every one of these reads as "not recorded": the drawing must still
        // say it, or a schedule that met the schema writes a deliverable that
        // does not.
        let scratch = scratch("unknowns");
        let schema_file = scratch.file(
            "schema.csv",
            "kind,attribute,value,detail,label\n\
             schema,Example Utility Schema,0.2,,\n\
             identifier,AssetIdentifier,,,\n\
             field,AssetIdentifier,Alphanumerical,Yes,Asset Identifier\n\
             field,LocateMethod,Domain List: Locate Method,Yes,Locate Method\n\
             domain,LocateMethod,Electronic Detection,,\n\
             field,QualityLevel,Domain List: Quality Level,Yes,Quality Level\n\
             domain,QualityLevel,Quality Level B,,\n\
             domain,QualityLevel,Unknown,,\n\
             field,Size,Alphanumerical,Yes,Size\n\
             field,AssetStatus,Domain List: Asset Status,Yes,Asset Status\n\
             domain,AssetStatus,In Service,,\n\
             domain,AssetStatus,Unknown,,\n\
             field,AssetTypeCode,Domain List: Asset Type Code,Yes,Asset Type Code\n\
             domain,AssetTypeCode,W,,\n\
             domain,AssetTypeCode,N,,\n\
             field,DepthLocation,Domain List: Depth Location,Yes,Depth Location\n\
             domain,DepthLocation,Top of Pipe,,\n\
             domain,DepthLocation,Unknown,,\n",
        );
        let schedule_text = "AssetIdentifier,point,easting,northing,LocateMethod,QualityLevel,Size,\
                             AssetStatus,AssetTypeCode,DepthLocation\n\
                             W-1,P1,334000,6250000,Electronic Detection,Unknown,Not Applicable,\
                             Unknown,N,Top of Pipe\n\
                             W-1,P2,334010,6250000,Electronic Detection,Unknown,Not Applicable,\
                             Unknown,N,Top of Pipe\n";
        let schedule_file = scratch.file("schedule.csv", schedule_text);
        let mut session = Session::default();
        let passes = "2 rows, 1 assets (AssetIdentifier): 0 errors, 0 warnings";
        let from_file = session.ok(&format!(
            "UTILITY CHECK {schedule_file} SCHEMA {schema_file}"
        ));
        assert!(contains(&from_file, passes), "{from_file}");

        session.ok(&format!("UTILITY DRAW {schedule_file}"));
        let from_drawing = session.ok(&format!("UTILITY CHECK DRAWING SCHEMA {schema_file}"));
        assert!(contains(&from_drawing, passes), "{from_drawing}");

        // The deliverable has every column the schema asks for, said as the
        // schedule said it, and reads back as the schedule read.
        session.ok(&format!(
            "UTILITY SCHEDULE \"{}\" DRAWING SCHEMA {schema_file}",
            scratch.at("out.csv")
        ));
        let written = scratch.read("out.csv");
        assert_eq!(
            first_line(&written),
            "AssetIdentifier,point,easting,northing,LocateMethod,DepthLocation,QualityLevel,\
             AssetTypeCode,Size,AssetStatus"
        );
        assert!(
            contains(
                &written,
                "\nW-1,P1,334000,6250000,Electronic Detection,Top of Pipe,Unknown,N,Not \
                 Applicable,Unknown\n"
            ),
            "{written}"
        );
        assert_eq!(parsed(&written), parsed(schedule_text));
    }

    // ---- the design CLEARANCE measures against --------------------------------------------

    #[test]
    fn an_entity_drawn_as_the_design_is_the_design_files_centre_line() {
        // design.csv's two vertices, drawn: a polyline with the heights a 3D
        // string carries.
        let mut drawn = Entity {
            id: 7,
            geometry: Geometry::Polyline(Polyline2::new(
                vec![
                    Point2::new(334015.0, 6249995.0),
                    Point2::new(334015.5, 6250010.0),
                ],
                false,
            )),
            ..Entity::default()
        };
        set_heights(&mut drawn.properties, &[Some(18.60), Some(18.45)]);
        let design = must(design_from_entity(&drawn, None));
        let file = must(parse_design_csv(
            &sample_text("design.csv"),
            String::from("#7"),
        ));
        assert_eq!(design.id, "#7");
        assert_eq!(design.vertices.len(), 2);
        for (made, read) in design.vertices.iter().zip(&file.vertices) {
            assert_eq!(made.position, read.position);
            assert_eq!(made.level, read.level);
        }
        // LEVEL is every vertex's; with neither there are none.
        let level = must(design_from_entity(&drawn, Some(17.5)));
        assert_eq!(level.vertices[1].level, Some(17.5));
        drawn.properties.clear();
        assert!(
            must(design_from_entity(&drawn, None)).vertices[0]
                .level
                .is_none()
        );

        // A closed polyline returns to its start; a line is its two ends.
        drawn.geometry = Geometry::Polyline(Polyline2::new(
            vec![
                Point2::new(0.0, 0.0),
                Point2::new(10.0, 0.0),
                Point2::new(10.0, 10.0),
            ],
            true,
        ));
        let ring = must(design_from_entity(&drawn, None));
        assert_eq!(ring.vertices.len(), 4);
        assert_eq!(ring.vertices[3].position, ring.vertices[0].position);
        drawn.geometry =
            Geometry::Line(Segment2::new(Point2::new(0.0, 0.0), Point2::new(3.0, 4.0)));
        assert_eq!(
            must(design_from_entity(&drawn, Some(1.0))).vertices.len(),
            2
        );
        // Nothing else is a centre line.
        drawn.geometry = Geometry::Circle(Circle2::new(Point2::new(0.0, 0.0), 5.0));
        let circle = design_from_entity(&drawn, None).expect_err("a circle is no centre line");
        // What is accepted, named: a curve polyline too, since the drawing
        // system (docs/drawing.md, "The merge into main").
        assert_eq!(
            circle.message,
            "entity #7 is circle; the design centre line is a line, a polyline or a curve \
             polyline"
        );
    }

    #[test]
    fn clearance_takes_the_design_from_an_entity_or_an_alignment() {
        let mut session = Session::default();
        let schedule = sample("schedule.csv");
        let file_row = [
            "W1 W1-2 -> W1-3",
            "QL-B",
            "within tolerance",
            "-0.263",
            "0.300",
        ];
        let from_file = session.ok(&format!(
            "UTILITY CLEARANCE {schedule} {} WIDTH 0.375",
            sample("design.csv")
        ));
        assert!(spaced_search(&from_file, &file_row), "{from_file}");

        // The centre line drawn, at one level.
        session.ok("PLINE 334015,6249995 334015.5,6250010");
        let pipe = session.document.last_created_entities()[0];
        let drawn_design = session.ok(&format!(
            "UTILITY CLEARANCE {schedule} DESIGN #{pipe} LEVEL 18.6 WIDTH 0.375"
        ));
        assert!(spaced_search(&drawn_design, &file_row), "{drawn_design}");

        // The same line as an alignment, its design profile falling 0.15 m
        // over it as design.csv's levels do.
        let mut alignment = Alignment {
            name: String::from("Storm 1"),
            horizontal: HorizontalAlignment {
                pis: vec![
                    pi_at(334015.0, 6249995.0, 0.0),
                    pi_at(334015.5, 6250010.0, 0.0),
                ],
                ..HorizontalAlignment::default()
            },
            ..Alignment::default()
        };
        let solved = must(solve_alignment(&alignment.horizontal));
        alignment.vertical = Some(VerticalAlignment {
            pvis: vec![pvi(0.0, 18.60, 0.0), pvi(solved.length(), 18.45, 0.0)],
        });
        ok(session
            .document
            .execute(create_alignment(alignment.clone())));
        let along_alignment = session.ok(&format!(
            "UTILITY CLEARANCE {schedule} DESIGN ALIGNMENT \"Storm 1\" WIDTH 0.375"
        ));
        let mut with_level = file_row.to_vec();
        with_level.push("0.372");
        assert!(
            spaced_search(&along_alignment, &with_level),
            "{along_alignment}"
        );
        let design = must(design_from_alignment(&alignment, DESIGN_CHORD_TOLERANCE));
        let file = must(parse_design_csv(
            &sample_text("design.csv"),
            String::from("Storm 1"),
        ));
        assert_eq!(design.vertices.len(), 2);
        for (made, read) in design.vertices.iter().zip(&file.vertices) {
            near(made.position.easting, read.position.easting, 1e-9);
            near(made.position.northing, read.position.northing, 1e-9);
            let (Some(made_level), Some(read_level)) = (made.level, read.level) else {
                panic!("a vertex with no level");
            };
            near(made_level, read_level, 1e-9);
        }

        assert!(contains(
            &session
                .refused(&format!(
                    "UTILITY CLEARANCE {schedule} {} LEVEL 18",
                    sample("design.csv")
                ))
                .describe(),
            "LEVEL is the level of a design drawn as an entity"
        ));
        assert!(contains(
            &session
                .refused(&format!("UTILITY CLEARANCE {schedule} DESIGN #99999"))
                .describe(),
            "no entity #99999"
        ));
        assert!(contains(
            &session
                .refused(&format!(
                    "UTILITY CLEARANCE {schedule} DESIGN ALIGNMENT nowhere"
                ))
                .describe(),
            "no alignment named nowhere"
        ));
        assert!(contains(
            &session
                .refused(&format!("UTILITY CLEARANCE {schedule} DESIGN"))
                .describe(),
            "usage: UTILITY CLEARANCE"
        ));
        assert!(contains(
            &session
                .refused(&format!("UTILITY CLEARANCE {schedule} DESIGN #x"))
                .describe(),
            "a design entity is #<entity id>"
        ));
        assert!(contains(
            &session
                .refused(&format!(
                    "UTILITY CLEARANCE {schedule} DESIGN #{pipe} LEVEL"
                ))
                .describe(),
            "LEVEL needs a level"
        ));
    }

    #[test]
    fn an_alignments_design_is_within_a_millimetre_of_its_curves_in_plan_and_in_level() {
        // A tangent, a 50 m curve and a tangent in plan; in level, a 60 m
        // crest curve between grades of +4 % and -3 %.
        let alignment = Alignment {
            name: String::from("Road"),
            horizontal: HorizontalAlignment {
                pis: vec![
                    pi_at(0.0, 0.0, 0.0),
                    pi_at(100.0, 0.0, 50.0),
                    pi_at(100.0, 100.0, 0.0),
                ],
                ..HorizontalAlignment::default()
            },
            vertical: Some(VerticalAlignment {
                pvis: vec![
                    pvi(0.0, 10.0, 0.0),
                    pvi(80.0, 13.2, 60.0),
                    pvi(170.0, 10.5, 0.0),
                ],
            }),
            ..Alignment::default()
        };
        let solved = solve_alignment(&alignment.horizontal);
        let Some(vertical) = alignment.vertical.as_ref() else {
            panic!("the alignment has no profile");
        };
        let profile = solve_profile(vertical);
        assert!(solved.is_ok() && profile.is_ok());
        let (solved, profile) = (must(solved), must(profile));
        let stations = design_stations(&alignment, DESIGN_CHORD_TOLERANCE);
        let design = design_from_alignment(&alignment, DESIGN_CHORD_TOLERANCE);
        assert!(stations.is_ok() && design.is_ok());
        let (stations, design) = (must(stations), must(design));
        assert_eq!(stations.len(), design.vertices.len());
        double_eq(stations[0], solved.start_station());
        double_eq(stations[stations.len() - 1], solved.end_station());

        // Each chord's middle against the curve at the middle station: the
        // sagitta in plan, and the parabola's departure in level - both
        // largest there.
        let mut on_the_arc = 0;
        let mut in_the_curve = 0;
        for (pair, ends) in stations.windows(2).zip(design.vertices.windows(2)) {
            let middle = 0.5 * (pair[0] + pair[1]);
            let (a, b) = (&ends[0], &ends[1]);
            let chord = Point2::new(
                0.5 * (a.position.easting + b.position.easting),
                0.5 * (a.position.northing + b.position.northing),
            );
            let Some(curve) = solved.point_at_station(middle) else {
                panic!("no point at {middle}");
            };
            assert!(
                (chord - curve).length() <= DESIGN_CHORD_TOLERANCE * 1.000001,
                "{middle}"
            );
            let level = profile.elevation_at(middle);
            if let (Some(level), Some(a_level), Some(b_level)) = (level, a.level, b.level) {
                assert!(
                    (0.5 * (a_level + b_level) - level).abs() <= DESIGN_CHORD_TOLERANCE * 1.000001,
                    "{middle}"
                );
            }
            let Some(element) = solved.element_at(middle) else {
                panic!("no element at {middle}");
            };
            if kind_of(element) == AlignmentElementKind::Arc {
                on_the_arc += 1;
            }
            if profile
                .element_at(middle)
                .is_some_and(|vertical| vertical.kind == ProfileElementKind::Curve)
            {
                in_the_curve += 1;
            }
        }
        // The checks reached the cases they are for.
        assert!(on_the_arc > 10);
        assert!(in_the_curve > 10);
        // The profile ends at 170 m, short of the alignment's end: no level
        // there.
        assert!(design.vertices[design.vertices.len() - 1].level.is_none());
        assert!(design.vertices[0].level.is_some());
    }

    #[test]
    fn an_alignments_design_ends_at_its_end_when_its_lengths_sum_a_rounding_past_it() {
        // A tangent, a tight curve and a 140 m tangent from chainage 43.45:
        // the last element's end, summed element by element from the start,
        // comes out one rounding past end_station(), start plus length. The
        // design must still run to the alignment's last PI - dropping that end
        // dropped the whole last tangent, and a service crossing it cleared.
        let alignment = Alignment {
            name: String::from("road"),
            horizontal: HorizontalAlignment {
                pis: vec![
                    pi_at(333950.0, 6250050.0, 0.0),
                    pi_at(333960.0, 6250000.3, 4.5958),
                    pi_at(334100.0, 6250000.3, 0.0),
                ],
                start_station: 43.45,
            },
            ..Alignment::default()
        };
        let solved = must(solve_alignment(&alignment.horizontal));
        let Some(last) = solved.elements().last() else {
            panic!("the alignment has no elements");
        };
        // The test reaches the case it is for.
        assert!(last.start_station + last.length > solved.end_station());

        let stations = design_stations(&alignment, DESIGN_CHORD_TOLERANCE);
        let design = design_from_alignment(&alignment, DESIGN_CHORD_TOLERANCE);
        assert!(stations.is_ok() && design.is_ok());
        let (stations, design) = (must(stations), must(design));
        assert_eq!(stations.len(), design.vertices.len());
        assert_eq!(stations[stations.len() - 1], solved.end_station());
        let end = &design.vertices[design.vertices.len() - 1];
        near(end.position.easting, 334100.0, 1e-6);
        near(end.position.northing, 6250000.3, 1e-6);
        near(design.vertices[0].position.easting, 333950.0, 1e-6);

        // The drawn water main crosses that tangent at W1-3 -> W1-4: a hard
        // conflict, whatever the chainage the alignment starts from.
        let mut session = Session::default();
        session.ok(&format!("UTILITY DRAW {}", sample("schedule.csv")));
        ok(session.document.execute(create_alignment(alignment)));
        let reply = session.ok("UTILITY CLEARANCE DRAWING DESIGN ALIGNMENT road");
        assert!(
            contains(&reply, "Suggested Clash attribute: W1 Hard"),
            "{reply}"
        );
    }

    #[test]
    fn help_utility_lists_the_drawing_sources_and_the_new_verbs() {
        let help = utility_verb_help();
        for word in [
            "UTILITY REGRADE <scope>",
            "UTILITY SCHEDULE <out.csv> <scope>",
            "<schedule.csv> | <scope>",
            "DESIGN",
            "#<id>",
            "LEVEL <z>",
            "ALIGNMENT <name>",
            "AREA x0,y0,x1,y1",
            "VIEW [id]",
            "WHERE",
            "completed=",
            "ignored=",
        ] {
            assert!(contains(&help, word), "{word}");
        }
        let general = CommandInterpreter::help_text();
        for word in ["VIEW [id]", "AREA x0,y0,x1,y1", "REGRADE", "SCHEDULE"] {
            assert!(contains(&general, word), "{word}");
        }
    }
}
