// UTILITY DRAW <scope>: the located services as a survey or an import left
// them in the drawing - strings and points, lines and polylines - drawn as
// services (read_geometry_services, utilities/utility_data.rs;
// docs/subsurface_utilities.md, "Services from surveyed and imported
// geometry"; tests/cad/utilities/test_utility_geometry.cpp). The owner asked
// on 2026-09-26 for the utility tools to act on "the data on the program":
// services are surveyed by GNSS or total station, or come as an archive, IFC,
// shapefile or DXF import, and are post-processed in Katana, not written into
// a schedule first.
//
// The proof the drawing is held to: a survey drawn as services is exactly
// what a schedule that says the same thing - written by hand below, row for
// row from the survey - draws, entity for entity. The schedule reader, and
// the grading it feeds, are held to hand-worked figures elsewhere
// (test_utility_verbs, the survey library's tests), so the survey is too.

mod utility_geometry {
    use katana_cad::Document;
    use katana_cad::utilities::utility_data::{
        GeometryHeights, GeometryServiceOptions, read_geometry_services,
    };
    use katana_cad::utilities::utility_drawing::keys;
    use katana_commands::{create_entities, set_entity_property};
    use katana_core::text::format_exact_real;
    use katana_entity::entity::{property_to_string, set_heights, to_string, type_name};
    use katana_entity::{
        Entity, EntityId, EntityType, Geometry, PointGeometry, PropertyMap, PropertyValue,
    };
    use katana_geometry::{Circle2, Point2, Polyline2, Segment2};
    use katana_survey::subsurface::{
        self as sub, LocationMethod, UtilityLine, UtilityStatus, UtilityType,
    };

    use crate::{ScratchFolder, Session, contains, first_line, must, ok, parsed, snapshot};

    fn scratch(name: &str) -> ScratchFolder {
        ScratchFolder::new(&format!("katana-utility-geometry-{name}"))
    }

    fn lines(text: &str) -> Vec<&str> {
        text.split('\n').collect()
    }

    fn line(text: &str, index: usize) -> String {
        String::from(lines(text).get(index).copied().unwrap_or(""))
    }

    fn without_line(text: &str, index: usize) -> String {
        let mut out = String::new();
        for (i, each) in lines(text).into_iter().enumerate() {
            if i != index {
                if !out.is_empty() {
                    out.push('\n');
                }
                out += each;
            }
        }
        out
    }

    // The text after the first line break; all of it when there is none (the
    // C++ `substr(find('\n') + 1)`, npos + 1 being 0).
    fn after_newline(text: &str) -> &str {
        match text.find('\n') {
            Some(at) => &text[at + 1..],
            None => text,
        }
    }

    fn props(pairs: Vec<(&str, PropertyValue)>) -> PropertyMap {
        pairs
            .into_iter()
            .map(|(key, value)| (String::from(key), value))
            .collect()
    }

    fn text(value: &str) -> PropertyValue {
        PropertyValue::from(value)
    }

    fn real(value: f64) -> PropertyValue {
        PropertyValue::from(value)
    }

    fn largest_id(document: &Document) -> EntityId {
        let mut largest: EntityId = 0;
        document
            .model()
            .entities
            .for_each(|each| largest = largest.max(each.id));
        largest
    }

    // `entity` added by its command; its id.
    #[track_caller]
    fn add(session: &mut Session, entity: Entity) -> EntityId {
        let before = largest_id(&session.document);
        ok(session.document.execute(create_entities(vec![entity])));
        let added = largest_id(&session.document);
        assert!(added > before);
        added
    }

    // A string as survey linework or an import leaves one: a polyline with
    // its heights, carrying `properties` (its code among them).
    #[track_caller]
    fn string(
        session: &mut Session,
        layer: &str,
        vertices: Vec<Point2>,
        heights: &[Option<f64>],
        properties: PropertyMap,
        closed: bool,
    ) -> EntityId {
        let mut entity = Entity {
            layer: String::from(layer),
            geometry: Geometry::Polyline(Polyline2::new(vertices, closed)),
            properties,
            ..Entity::default()
        };
        set_heights(&mut entity.properties, heights);
        add(session, entity)
    }

    // A point as the survey import leaves one: its number and its height.
    #[track_caller]
    fn point(
        session: &mut Session,
        layer: &str,
        at: Point2,
        number: &str,
        height: Option<f64>,
        properties: PropertyMap,
    ) -> EntityId {
        let mut entity = Entity {
            layer: String::from(layer),
            geometry: Geometry::Point(PointGeometry { position: at }),
            properties,
            ..Entity::default()
        };
        if !number.is_empty() {
            entity
                .properties
                .insert(String::from("point"), PropertyValue::from(number));
        }
        set_heights(&mut entity.properties, &[height]);
        add(session, entity)
    }

    fn all(session: &Session) -> Vec<EntityId> {
        let mut ids = Vec::new();
        session
            .document
            .model()
            .entities
            .for_each(|entity| ids.push(entity.id));
        ids
    }

    fn p(x: f64, y: f64) -> Point2 {
        Point2::new(x, y)
    }

    // What UTILITY DRAW made, entity by entity, as values that do not depend
    // on ids: the entities under "utilities", with the provenance a draw from
    // the drawing adds (keys::SOURCE) left out, since a schedule has none.
    fn drawn(document: &Document) -> Vec<String> {
        let mut entities = Vec::new();
        document.model().entities.for_each(|entity| {
            if !entity.layer.starts_with("utilities") {
                return;
            }
            let mut text = format!("{}|{}", entity.layer, to_string(entity.r#type()));
            match &entity.geometry {
                Geometry::Point(point) => {
                    text += &format!(
                        " {},{}",
                        format_exact_real(point.position.x),
                        format_exact_real(point.position.y)
                    );
                }
                Geometry::Polyline(run) => {
                    for vertex in &run.vertices {
                        text += &format!(
                            " {},{}",
                            format_exact_real(vertex.x),
                            format_exact_real(vertex.y)
                        );
                    }
                }
                _ => {}
            }
            for (key, value) in &entity.properties {
                if key != keys::SOURCE {
                    text += &format!("|{key}={}:{}", type_name(value), property_to_string(value));
                }
            }
            entities.push(text);
        });
        entities.sort();
        entities
    }

    // ---- the survey -----------------------------------------------------------------------
    //
    // A water main detected by EML and marked on the road, the marks shot by
    // GNSS - so its heights are the surface - with the locator's depth read at
    // two marks; two potholes on it shot on the pipe, the first an open trench
    // to the second and checking the detection at 102. An electricity duct
    // picked by radar, whose string was reduced to the duct's top levels.
    // Written out below as the schedule a person would type from the same
    // field book.

    const SURVEY_SCHEDULE: &str = "line,point,easting,northing,method,level,level_ref,surface,\
        depth,h_unc,v_unc,ql,path,verifies,type,owner,material,diameter_mm,status\n\
        W1,101,334000,6250000,EML,,,20.25,1.05,0.1,,QL-B,,,water,WaterCo,DICL,150,in service\n\
        W1,102,334008,6250000.15,EML,,,20.22,1.02,0.1,0.35,QL-B,,,,,,,\n\
        W1,103,334016.08,6250000.31,pothole,19.155,top,20.22,,0.02,0.02,QL-A,exposed,102,,,,,\n\
        W1,104,334017.5,6250000.3,pothole,19.15,top,20.21,,0.02,0.02,QL-A,,,,,,,\n\
        W1,105,334030,6250000.5,EML,,,20.2,,0.1,,,,,,,,,\n\
        E1,201,334000,6250003,GPR,19.6,top,,,0.25,0.45,,,,electricity,PowerCo,,,\n\
        E1,202,334009,6250003.1,GPR,19.55,top,,,0.25,0.45,,,,,,,,\n";

    // The survey in the drawing, as a coded survey leaves it: the strings on
    // survey/strings named by their code, the points on survey/points. What
    // the geometry cannot say is on the entities as utility.* properties - how
    // the potholes were found, the locator's depths - or is left to the verb's
    // options (the W1 main's method, uncertainty, type and owner).
    struct Survey {
        water: EntityId,
        duct: EntityId,
        points: Vec<EntityId>,
    }

    fn draw_survey(session: &mut Session) -> Survey {
        session.ok("LAYER NEW survey/strings");
        session.ok("LAYER NEW survey/points");
        let water = [
            p(334000.0, 6250000.0),
            p(334008.0, 6250000.15),
            p(334016.08, 6250000.31),
            p(334017.5, 6250000.3),
            p(334030.0, 6250000.5),
        ];
        // The potholes' shots are on the pipe, so the string's heights there
        // are the pipe's.
        let water_heights = [
            Some(20.25),
            Some(20.22),
            Some(19.155),
            Some(19.15),
            Some(20.2),
        ];
        let water_id = string(
            session,
            "survey/strings",
            water.to_vec(),
            &water_heights,
            props(vec![
                ("code", text("W1")),
                ("utility.material", text("DICL")),
                ("utility.diameter", real(0.15)),
                ("utility.status", text("in service")),
            ]),
            false,
        );
        let duct = [p(334000.0, 6250003.0), p(334009.0, 6250003.1)];
        let duct_id = string(
            session,
            "survey/strings",
            duct.to_vec(),
            &[Some(19.6), Some(19.55)],
            props(vec![
                ("code", text("E1")),
                ("utility.type", text("electricity")),
                ("utility.owner", text("PowerCo")),
                ("utility.method", text("GPR")),
                ("utility.h_unc", real(0.25)),
                ("utility.v_unc", real(0.45)),
                ("utility.heights", text("service")),
            ]),
            false,
        );

        let pothole = props(vec![
            ("utility.method", text("pothole")),
            ("utility.heights", text("service")),
            ("utility.h_unc", real(0.02)),
            ("utility.v_unc", real(0.02)),
            ("utility.claimed", text("QL-A")),
        ]);
        let mut trench = pothole.clone();
        trench.insert(String::from("utility.surface_level"), real(20.22));
        trench.insert(String::from("utility.path"), text("exposed"));
        trench.insert(String::from("utility.verifies"), text("102"));
        let mut second = pothole;
        second.insert(String::from("utility.surface_level"), real(20.21));
        let water_points = [
            (
                "101",
                props(vec![
                    ("code", text("W1")),
                    ("utility.depth", real(1.05)),
                    ("utility.claimed", text("QL-B")),
                ]),
            ),
            (
                "102",
                props(vec![
                    ("code", text("W1")),
                    ("utility.depth", real(1.02)),
                    ("utility.v_unc", real(0.35)),
                    ("utility.claimed", text("QL-B")),
                ]),
            ),
            ("103", trench),
            ("104", second),
            ("105", props(vec![("code", text("W1"))])),
        ];
        let mut points = Vec::new();
        for ((&at, &height), (number, properties)) in
            water.iter().zip(&water_heights).zip(water_points)
        {
            points.push(point(
                session,
                "survey/points",
                at,
                number,
                height,
                properties,
            ));
        }
        points.push(point(
            session,
            "survey/points",
            duct[0],
            "201",
            Some(19.6),
            PropertyMap::new(),
        ));
        points.push(point(
            session,
            "survey/points",
            duct[1],
            "202",
            Some(19.55),
            PropertyMap::new(),
        ));
        Survey {
            water: water_id,
            duct: duct_id,
            points,
        }
    }

    const DRAW_SURVEY: &str =
        "UTILITY DRAW LAYERS survey TYPE water METHOD EML H_UNC 0.1 OWNER WaterCo";

    // ---- the survey is the schedule -------------------------------------------------------

    #[test]
    fn the_survey_read_as_services_is_the_schedule_of_the_same_survey() {
        let mut session = Session::default();
        let survey = draw_survey(&mut session);
        let options = GeometryServiceOptions {
            r#type: Some(UtilityType::Water),
            method: Some(LocationMethod::ElectromagneticLocation),
            horizontal_uncertainty: Some(0.1),
            owner: String::from("WaterCo"),
            ..GeometryServiceOptions::default()
        };
        let scope = all(&session);
        let services = must(read_geometry_services(
            session.document.model(),
            &scope,
            &options,
        ));
        assert_eq!(services.lines, parsed(SURVEY_SCHEDULE));
        assert_eq!(services.sources, vec![survey.water, survey.duct]);
        assert_eq!(services.points, 7);
        assert_eq!(services.loose, 0);
        assert_eq!(services.drawn, 0);
        assert_eq!(services.ignored, 0);
    }

    #[test]
    fn a_survey_drawn_as_services_draws_what_the_schedule_of_the_same_survey_draws() {
        let scratch = scratch("same");
        let schedule = scratch.file("survey.csv", SURVEY_SCHEDULE);
        let mut from_schedule = Session::default();
        let schedule_reply = from_schedule.ok(&format!("UTILITY DRAW {schedule}"));

        let mut from_survey = Session::default();
        draw_survey(&mut from_survey);
        let reply = from_survey.ok(DRAW_SURVEY);
        assert_eq!(drawn(&from_survey.document), drawn(&from_schedule.document));
        // The draw's records, with what the scope took after the first: two
        // strings and seven points, each point on a vertex.
        assert_eq!(first_line(&reply), first_line(&schedule_reply));
        assert_eq!(
            line(&reply, 1),
            "scope=layers layers=survey sublayers=yes matched=9 lines=2 points=7 loose=0 \
             drawn=0 ignored=0"
        );
        assert_eq!(without_line(&reply, 1), schedule_reply);
        assert!(
            contains(&schedule_reply, "line id=W1 type=water"),
            "{schedule_reply}"
        );
        assert!(
            contains(&schedule_reply, "line id=E1 type=electricity"),
            "{schedule_reply}"
        );
    }

    #[test]
    fn every_verb_reads_what_was_drawn_from_the_survey_as_the_schedules_drawing() {
        let scratch = scratch("verbs");
        let schedule = scratch.file("survey.csv", SURVEY_SCHEDULE);
        let mut session = Session::default();
        draw_survey(&mut session);
        session.ok(DRAW_SURVEY);
        let report = session.ok("UTILITY REPORT LAYERS utilities MINCOVER 0.6");
        assert_eq!(
            after_newline(&report),
            session.ok(&format!("UTILITY REPORT {schedule} MINCOVER 0.6"))
        );
        let verify = session.ok("UTILITY VERIFY LAYERS utilities");
        assert_eq!(
            after_newline(&verify),
            session.ok(&format!("UTILITY VERIFY {schedule}"))
        );

        // Written back, it is the schedule of the survey.
        session.ok(&format!(
            "UTILITY SCHEDULE \"{}\" LAYERS utilities",
            scratch.at("written.csv")
        ));
        assert_eq!(
            parsed(&scratch.read("written.csv")),
            parsed(SURVEY_SCHEDULE)
        );

        // Nothing was edited, so a regrade changes nothing - and the points
        // keep the strings they were drawn from, which is what says they are
        // drawn.
        let steps = session.steps();
        assert!(contains(
            &first_line(&session.ok("UTILITY REGRADE LAYERS utilities")),
            "utilities regraded changed=0 "
        ));
        assert_eq!(session.steps(), steps);
        let mut marked = 0;
        session.document.model().entities.for_each(|entity| {
            if entity.r#type() == EntityType::Point && entity.properties.contains_key(keys::LINE) {
                assert!(
                    entity.properties.contains_key(keys::SOURCE),
                    "{}",
                    entity.id
                );
                marked += 1;
            }
        });
        assert_eq!(marked, 7);
    }

    #[test]
    fn a_draw_of_geometry_is_one_undo_step_and_leaves_the_survey_as_it_was() {
        let mut session = Session::default();
        let survey = draw_survey(&mut session);
        let before = snapshot(&session.document);
        let layers = session.document.model().layers.size();
        let steps = session.steps();
        session.ok(DRAW_SURVEY);
        assert_eq!(session.steps(), steps + 1);
        let after = snapshot(&session.document);
        for (id, entity) in &before {
            assert!(after.contains_key(id));
            assert_eq!(after[id], *entity, "#{id} was changed");
        }
        assert!(after.len() > before.len());
        ok(session.document.undo());
        assert_eq!(snapshot(&session.document), before);
        assert_eq!(session.document.model().layers.size(), layers);
        assert_ne!(survey.water, 0);
    }

    // ---- what each value is taken from ----------------------------------------------------

    // The first line read from `scope` with the heights taken as `heights`.
    #[track_caller]
    fn read_with(session: &Session, scope: &[EntityId], heights: GeometryHeights) -> UtilityLine {
        let options = GeometryServiceOptions {
            method: Some(LocationMethod::ElectromagneticLocation),
            heights,
            ..GeometryServiceOptions::default()
        };
        let services = must(read_geometry_services(
            session.document.model(),
            scope,
            &options,
        ));
        match services.lines.into_iter().next() {
            Some(line) => line,
            None => panic!("no line was read"),
        }
    }

    #[test]
    fn heights_are_the_surface_unless_something_says_otherwise() {
        let mut session = Session::default();
        session.ok("LAYER NEW survey");
        let run = string(
            &mut session,
            "survey",
            vec![p(0.0, 0.0), p(10.0, 0.0), p(20.0, 0.0)],
            &[Some(20.0), None, Some(22.0)],
            props(vec![("code", text("S1"))]),
            false,
        );
        // The string has no height at its middle; the point there has one.
        point(
            &mut session,
            "survey",
            p(10.0, 0.0),
            "",
            Some(21.0),
            PropertyMap::new(),
        );
        let scope = all(&session);

        let surface = read_with(&session, &scope, GeometryHeights::Surface);
        assert_eq!(surface.vertices.len(), 3);
        let expected = [20.0, 21.0, 22.0];
        for (i, (vertex, level)) in surface.vertices.iter().zip(expected).enumerate() {
            assert_eq!(vertex.surface_level, Some(level), "{i}");
            assert!(vertex.level.is_none(), "{i}");
        }
        let service = read_with(&session, &scope, GeometryHeights::Service);
        for (i, (vertex, level)) in service.vertices.iter().zip(expected).enumerate() {
            assert_eq!(vertex.level, Some(level), "{i}");
            assert!(vertex.surface_level.is_none(), "{i}");
            assert!(vertex.evidence.has_level, "{i}");
        }
        let none = read_with(&session, &scope, GeometryHeights::Unused);
        for vertex in &none.vertices {
            assert!(vertex.level.is_none());
            assert!(vertex.surface_level.is_none());
        }

        // The line says it for all its vertices over the option; a point for
        // its own.
        ok(session.document.execute(set_entity_property(
            vec![run],
            "utility.heights",
            text("service"),
        )));
        let told = read_with(&session, &scope, GeometryHeights::Surface);
        assert_eq!(told.vertices[0].level, Some(20.0));
        assert_eq!(told.vertices[2].level, Some(22.0));
        // A string with no heights is a plan string: no level, no surface.
        session.ok("LAYER NEW flat");
        let plan = string(
            &mut session,
            "flat",
            vec![p(0.0, 50.0), p(10.0, 50.0)],
            &[None, None],
            props(vec![("code", text("F1"))]),
            false,
        );
        let options = GeometryServiceOptions {
            method: Some(LocationMethod::Records),
            ..GeometryServiceOptions::default()
        };
        let flat = must(read_geometry_services(
            session.document.model(),
            &[plan],
            &options,
        ));
        for vertex in &flat.lines[0].vertices {
            assert!(vertex.level.is_none());
            assert!(vertex.surface_level.is_none());
            assert!(!vertex.evidence.has_level);
        }
    }

    #[test]
    fn the_points_own_value_wins_over_the_lines_and_the_lines_over_the_options() {
        let mut session = Session::default();
        session.ok("LAYER NEW survey");
        string(
            &mut session,
            "survey",
            vec![p(0.0, 0.0), p(5.0, 0.0), p(10.0, 0.0)],
            &[],
            props(vec![
                ("code", text("G1")),
                ("utility.type", text("gas")),
                ("utility.method", text("EML")),
            ]),
            false,
        );
        point(
            &mut session,
            "survey",
            p(5.0, 0.0),
            "7",
            None,
            props(vec![
                ("utility.method", text("GPR")),
                ("utility.h_unc", real(0.05)),
                // A point says nothing of its line: the line is the gas main.
                ("utility.type", text("water")),
                ("utility.owner", text("Someone")),
            ]),
        );
        let options = GeometryServiceOptions {
            r#type: Some(UtilityType::Electricity),
            method: Some(LocationMethod::Records),
            horizontal_uncertainty: Some(0.3),
            owner: String::from("GasCo"),
            ..GeometryServiceOptions::default()
        };
        let scope = all(&session);
        let services = must(read_geometry_services(
            session.document.model(),
            &scope,
            &options,
        ));
        let gas = &services.lines[0];
        assert_eq!(gas.id, "G1");
        assert_eq!(gas.attributes.r#type, UtilityType::Gas);
        assert_eq!(gas.attributes.owner, "GasCo");
        assert_eq!(gas.vertices.len(), 3);
        assert_eq!(
            gas.vertices[0].evidence.method,
            LocationMethod::ElectromagneticLocation
        );
        assert_eq!(
            gas.vertices[1].evidence.method,
            LocationMethod::GroundPenetratingRadar
        );
        assert_eq!(
            gas.vertices[2].evidence.method,
            LocationMethod::ElectromagneticLocation
        );
        assert_eq!(gas.vertices[0].evidence.horizontal_uncertainty, Some(0.3));
        assert_eq!(gas.vertices[1].evidence.horizontal_uncertainty, Some(0.05));
        // The point gives its vertex its number; the others are numbered
        // along the line.
        assert_eq!(gas.vertices[0].id, "G1-1");
        assert_eq!(gas.vertices[1].id, "7");
        assert_eq!(gas.vertices[2].id, "G1-3");
    }

    #[test]
    fn a_service_is_named_by_its_code_and_two_of_one_name_are_told_apart_by_their_entities() {
        let mut session = Session::default();
        session.ok("LAYER NEW imported");
        let coded = string(
            &mut session,
            "imported",
            vec![p(0.0, 0.0), p(10.0, 0.0)],
            &[],
            props(vec![("code", text("WM01"))]),
            false,
        );
        let first = string(
            &mut session,
            "imported",
            vec![p(0.0, 10.0), p(10.0, 10.0)],
            &[],
            props(vec![("CODE", text("WATER"))]),
            false,
        );
        let second = string(
            &mut session,
            "imported",
            vec![p(0.0, 20.0), p(10.0, 20.0)],
            &[],
            props(vec![("CODE", text("WATER"))]),
            false,
        );
        let mut archived = Entity {
            layer: String::from("imported"),
            geometry: Geometry::Line(Segment2::new(p(0.0, 30.0), p(10.0, 30.0))),
            ..Entity::default()
        };
        archived
            .metadata
            .insert(String::from("12d.name"), text("GAS 1"));
        let from_archive = add(&mut session, archived);
        let unnamed = string(
            &mut session,
            "imported",
            vec![p(0.0, 40.0), p(10.0, 40.0)],
            &[],
            PropertyMap::new(),
            false,
        );

        let reply = session.ok("UTILITY DRAW LAYERS imported METHOD records");
        for id in [
            String::from("WM01"),
            format!("WATER#{first}"),
            format!("WATER#{second}"),
            String::from("\"GAS 1\""),
            format!("#{unnamed}"),
        ] {
            assert!(contains(&reply, &format!("line id={id} ")), "{id}\n{reply}");
        }
        assert_ne!(coded, from_archive);

        // A service drawn already keeps its name; a new one of that name is
        // told apart from it.
        let another = string(
            &mut session,
            "imported",
            vec![p(0.0, 50.0), p(10.0, 50.0)],
            &[],
            props(vec![("code", text("WM01"))]),
            false,
        );
        let again = session.ok("UTILITY DRAW LAYERS imported METHOD records");
        assert!(contains(&again, "lines=1 "), "{again}");
        assert!(
            contains(&again, &format!("line id=WM01#{another} ")),
            "{again}"
        );
    }

    // ---- what the scope took --------------------------------------------------------------

    #[test]
    fn a_draw_of_the_same_scope_again_draws_only_what_is_new() {
        let mut session = Session::default();
        draw_survey(&mut session);
        session.ok(DRAW_SURVEY);
        let steps = session.steps();
        let before = snapshot(&session.document);

        // Everything in the survey's layers is drawn already: the two strings
        // and their seven points.
        let again = session.ok(DRAW_SURVEY);
        assert_eq!(
            again,
            "scope=layers layers=survey sublayers=yes matched=9 lines=0 points=0 loose=0 \
             drawn=9 ignored=0\n\
             no lines or open polylines in the scope that are not drawn already: nothing drawn"
        );
        assert_eq!(session.steps(), steps);
        assert_eq!(snapshot(&session.document), before);

        // The whole drawing: the drawn services' runs and points are drawn
        // too.
        let everything = session.ok("UTILITY DRAW DRAWING METHOD EML");
        assert!(
            contains(
                &everything,
                &format!(" lines=0 points=0 loose=0 drawn={} ignored=0", before.len())
            ),
            "{everything}"
        );

        // A string surveyed since is drawn, and only it.
        string(
            &mut session,
            "survey/strings",
            vec![p(334000.0, 6250010.0), p(334010.0, 6250010.0)],
            &[],
            props(vec![("code", text("T1"))]),
            false,
        );
        let surveyed = session.steps();
        let added = session.ok(DRAW_SURVEY);
        assert!(
            contains(
                &added,
                "matched=10 lines=1 points=0 loose=0 drawn=9 ignored=0"
            ),
            "{added}"
        );
        assert!(contains(&added, "line id=T1 type=water"), "{added}");
        assert_eq!(session.steps(), surveyed + 1);
    }

    #[test]
    fn what_cannot_be_a_service_is_ignored_and_a_point_on_no_line_is_loose() {
        let mut session = Session::default();
        session.ok("LAYER NEW site");
        string(
            &mut session,
            "site",
            vec![p(0.0, 0.0), p(10.0, 0.0)],
            &[],
            props(vec![("code", text("S1"))]),
            false,
        );
        // An outline, not a run.
        string(
            &mut session,
            "site",
            vec![p(0.0, 5.0), p(5.0, 5.0), p(5.0, 10.0)],
            &[],
            props(vec![("code", text("PIT"))]),
            true,
        );
        let circle = Entity {
            layer: String::from("site"),
            geometry: Geometry::Circle(Circle2::new(p(20.0, 20.0), 1.0)),
            ..Entity::default()
        };
        add(&mut session, circle);
        point(
            &mut session,
            "site",
            p(3.0, 3.0),
            "900",
            None,
            PropertyMap::new(),
        );
        let reply = session.ok("UTILITY DRAW LAYERS site METHOD records");
        assert_eq!(
            line(&reply, 1),
            "scope=layers layers=site sublayers=yes matched=4 lines=1 points=0 loose=1 drawn=0 \
             ignored=2"
        );

        // A scope with nothing to draw says so, and is no failure.
        let mut empty = Session::default();
        assert_eq!(
            empty.ok("UTILITY DRAW DRAWING METHOD EML"),
            "scope=drawing matched=0 lines=0 points=0 loose=0 drawn=0 ignored=0\n\
             no lines or open polylines in the scope that are not drawn already: nothing drawn"
        );
        assert_eq!(empty.steps(), 0);
    }

    #[test]
    fn a_report_of_survey_geometry_says_how_to_draw_it_as_services() {
        let mut session = Session::default();
        draw_survey(&mut session);
        let reply = session.ok("UTILITY REPORT LAYERS survey");
        assert_eq!(
            first_line(&reply),
            "scope=layers layers=survey sublayers=yes matched=9 lines=0 completed=0 ignored=9"
        );
        assert!(
            contains(
                &reply,
                "no utility lines in the scope: nothing to report\n\
                 what the scope took carries no utility data; UTILITY DRAW with the same scope \
                 and METHOD <method> draws"
            ),
            "{reply}"
        );
    }

    // ---- refused, by name, with nothing changed ---------------------------------------------

    fn set_on(session: &mut Session, id: EntityId, key: &str, value: &str) {
        ok(session
            .document
            .execute(set_entity_property(vec![id], key, text(value))));
    }

    #[test]
    fn what_cannot_be_read_is_refused_naming_the_entity_and_nothing_changes() {
        let mut session = Session::default();
        let survey = draw_survey(&mut session);
        let before = snapshot(&session.document);
        let steps = session.steps();

        // No method anywhere for the main's marks.
        let why = session
            .refused("UTILITY DRAW LAYERS survey TYPE water")
            .describe();
        assert!(
            contains(
                &why,
                &format!(
                    "line W1: vertex 101 (#{}) says no method it was located by; give METHOD",
                    survey.points[0]
                )
            ),
            "{why}"
        );
        assert_eq!(snapshot(&session.document), before);
        assert_eq!(session.steps(), steps);

        // A value on the string that does not read is refused by the string.
        set_on(&mut session, survey.duct, "utility.method", "dowsing");
        let why = session.refused(DRAW_SURVEY).describe();
        assert!(
            contains(
                &why,
                &format!(
                    "line E1: polyline (#{}) has utility.method \"dowsing\", which is not a \
                     location method",
                    survey.duct
                )
            ),
            "{why}"
        );
        ok(session.document.undo());

        // And one on a point, by the point.
        set_on(&mut session, survey.points[1], "utility.depth", "deep");
        let why = session.refused(DRAW_SURVEY).describe();
        assert!(
            contains(
                &why,
                &format!(
                    "line W1: point 102 (#{}) has utility.depth \"deep\", which is not a number",
                    survey.points[1]
                )
            ),
            "{why}"
        );
        ok(session.document.undo());
        set_on(&mut session, survey.points[1], "utility.heights", "sky");
        let why = session.refused(DRAW_SURVEY).describe();
        assert!(
            contains(
                &why,
                "has utility.heights \"sky\", which is not surface, service or none"
            ),
            "{why}"
        );
        ok(session.document.undo());
        assert_eq!(snapshot(&session.document), before);
        assert_eq!(session.steps(), steps);

        // Two points on one vertex: which is the survey's is not guessed.
        let twin = point(
            &mut session,
            "survey/points",
            p(334008.0, 6250000.15),
            "102A",
            Some(20.22),
            PropertyMap::new(),
        );
        let why = session.refused(DRAW_SURVEY).describe();
        assert!(
            contains(
                &why,
                &format!(
                    "line W1: vertex 2 of polyline (#{}) has two points on it, #{} and #{twin}",
                    survey.water, survey.points[1]
                )
            ),
            "{why}"
        );
        ok(session.document.undo());

        // A point that is a vertex of a schedule and lost its line.
        set_on(&mut session, survey.points[4], "utility.vertex", "W1-5");
        let why = session.refused(DRAW_SURVEY).describe();
        assert!(
            contains(&why, "is a vertex of a schedule and has no utility.line"),
            "{why}"
        );
        ok(session.document.undo());
        assert_eq!(snapshot(&session.document), before);
        assert_eq!(session.steps(), steps);

        // A layer the draw would use, locked: refused when it runs, and the
        // layers made before it taken back.
        session.ok("LAYER NEW utilities/water/QL-A");
        session.ok("LAYER LOCK utilities/water/QL-A");
        let locked = snapshot(&session.document);
        let layers = session.document.model().layers.size();
        let why = session.refused(DRAW_SURVEY).describe();
        assert!(contains(&why, "locked"), "{why}");
        assert_eq!(snapshot(&session.document), locked);
        assert_eq!(session.document.model().layers.size(), layers);
    }

    // Why the draw of the survey with METHOD EML and `words` is refused.
    #[track_caller]
    fn draw_refusal(session: &mut Session, words: &str) -> String {
        session
            .refused(&format!("UTILITY DRAW LAYERS survey METHOD EML {words}"))
            .describe()
    }

    #[test]
    fn the_options_are_refused_by_name_when_they_do_not_read() {
        let mut session = Session::default();
        draw_survey(&mut session);
        let steps = session.steps();
        assert!(contains(
            &draw_refusal(&mut session, "TYPE steam"),
            "TYPE steam is not a utility type"
        ));
        assert!(contains(
            &session
                .refused("UTILITY DRAW LAYERS survey METHOD dowsing")
                .describe(),
            "METHOD dowsing is not a location method"
        ));
        assert!(contains(
            &draw_refusal(&mut session, "HEIGHTS sky"),
            "HEIGHTS sky is not surface, service or none"
        ));
        assert!(contains(
            &draw_refusal(&mut session, "LEVEL_REF side"),
            "LEVEL_REF side is not a level reference"
        ));
        assert!(contains(
            &draw_refusal(&mut session, "PATH flown"),
            "PATH flown is not detected, exposed or assumed"
        ));
        assert!(contains(
            &draw_refusal(&mut session, "STATUS gone"),
            "STATUS gone is not a status"
        ));
        assert!(contains(
            &draw_refusal(&mut session, "DIAMETER_MM 0"),
            "DIAMETER_MM needs a diameter in millimetres, more than 0"
        ));
        assert!(contains(
            &draw_refusal(&mut session, "H_UNC -0.1"),
            "H_UNC needs a horizontal uncertainty, in metres, not negative"
        ));
        assert!(contains(
            &draw_refusal(&mut session, "OWNER"),
            "OWNER needs the owner's name"
        ));
        assert!(contains(
            &draw_refusal(&mut session, "TYPE water TYPE gas"),
            "TYPE given twice"
        ));
        assert!(contains(
            &draw_refusal(&mut session, "COLOUR red"),
            "unknown option COLOUR"
        ));
        // A schedule says these in its columns.
        assert!(contains(
            &session
                .refused("UTILITY DRAW schedule.csv METHOD EML")
                .describe(),
            "METHOD is for a draw of geometry in the drawing (UTILITY DRAW <scope> ...); a \
             schedule gives its own in its columns"
        ));
        assert_eq!(session.steps(), steps);
    }

    #[test]
    fn every_option_reaches_the_drawing() {
        let mut session = Session::default();
        session.ok("LAYER NEW dxf");
        string(
            &mut session,
            "dxf",
            vec![p(0.0, 0.0), p(6.0, 0.0), p(12.0, 0.0)],
            &[Some(1.5), Some(1.4), Some(1.3)],
            props(vec![("code", text("S7"))]),
            false,
        );
        let reply = session.ok(
            "UTILITY DRAW LAYERS dxf TYPE sewer METHOD pothole H_UNC 0.03 V_UNC 0.04 HEIGHTS \
             service LEVEL_REF invert PATH exposed OWNER \"Water Board\" MATERIAL VC DIAMETER_MM \
             225 STATUS abandoned SPACING 5 MINCOVER 0.9 LAYER located",
        );
        assert!(contains(&reply, "line id=S7 type=sewer"), "{reply}");
        let mut points: Vec<Entity> = Vec::new();
        session.document.model().entities.for_each(|entity| {
            if entity.r#type() == EntityType::Point && entity.layer == "located/sewer/points" {
                points.push(entity.clone());
            }
        });
        assert_eq!(points.len(), 3);
        let first = &points[0].properties;
        let value = |key: &str| {
            first
                .get(key)
                .map_or_else(|| String::from("(absent)"), property_to_string)
        };
        assert_eq!(value(keys::TYPE), "sewer");
        assert_eq!(
            value(keys::METHOD),
            sub::to_string(LocationMethod::NonDestructiveExcavation)
        );
        assert_eq!(value(keys::HORIZONTAL_UNCERTAINTY), "0.03");
        assert_eq!(value(keys::VERTICAL_UNCERTAINTY), "0.04");
        assert_eq!(value(keys::LEVEL), "1.5");
        assert_eq!(value(keys::SURFACE_LEVEL), "(absent)");
        assert_eq!(value(keys::LEVEL_REFERENCE), "invert");
        assert_eq!(value(keys::PATH), "exposed");
        assert_eq!(value(keys::OWNER), "Water Board");
        assert_eq!(value(keys::MATERIAL), "VC");
        assert_eq!(value(keys::DIAMETER), "0.225");
        assert_eq!(
            value(keys::STATUS),
            sub::to_string(UtilityStatus::Abandoned)
        );
        assert_eq!(value(keys::SPACING), "5");
        assert_eq!(value(keys::MINIMUM_COVER), "0.9");
    }

    // ---- an import's own attributes -------------------------------------------------------

    #[test]
    fn fields_read_an_imports_own_attributes_as_the_schedules_columns() {
        // A water authority's shapefile of its mains as an import leaves it:
        // its attributes under its own names. Its asset id, type code, locate
        // method (as AS 5488.2 names one), a depth for the whole main, a
        // diameter in millimetres, one of the delivery schema's own attributes
        // - and an owner a person has corrected in Katana since, which wins.
        let mut session = Session::default();
        session.ok("LAYER NEW gis");
        let main = string(
            &mut session,
            "gis",
            vec![p(0.0, 0.0), p(8.0, 0.0), p(16.0, 0.0)],
            &[],
            props(vec![
                ("ASSET_ID", text("W-2231")),
                ("ASSET_TYPE", text("W")),
                ("LOC_METHOD", text("Electronic Detection")),
                ("DEPTH", real(0.9)),
                ("DIA", PropertyValue::Integer(150)),
                ("OWNER", text("Old Water Board")),
                ("utility.owner", text("WaterCo")),
                ("FEAT", text("Main")),
            ]),
            false,
        );
        point(
            &mut session,
            "gis",
            p(8.0, 0.0),
            "",
            None,
            props(vec![("PT_ID", text("V17"))]),
        );
        let reply = session.ok(
            "UTILITY DRAW LAYERS gis H_UNC 0.2 FIELDS \"line=ASSET_ID, type=ASSET_TYPE, \
             LocateMethod=LOC_METHOD,depth=DEPTH,diameter_mm=DIA,owner=OWNER,point=PT_ID,\
             AssetFeature=FEAT\"",
        );
        assert!(contains(&reply, "line id=W-2231 type=water"), "{reply}");
        assert_ne!(main, 0);

        let mut by_vertex: std::collections::BTreeMap<String, PropertyMap> =
            std::collections::BTreeMap::new();
        session.document.model().entities.for_each(|entity| {
            if entity.r#type() == EntityType::Point && entity.layer == "utilities/water/points" {
                let Some(vertex) = entity.properties.get(keys::VERTEX) else {
                    panic!("#{} has no {}", entity.id, keys::VERTEX);
                };
                by_vertex
                    .entry(property_to_string(vertex))
                    .or_insert_with(|| entity.properties.clone());
            }
        });
        assert_eq!(by_vertex.len(), 3);
        assert!(by_vertex.contains_key("V17"));
        let Some(first) = by_vertex.get("W-2231-1") else {
            panic!("no vertex W-2231-1");
        };
        let value = |key: &str| {
            first
                .get(key)
                .map_or_else(|| String::from("(absent)"), property_to_string)
        };
        assert_eq!(
            value(keys::METHOD),
            sub::to_string(LocationMethod::ElectromagneticLocation)
        );
        assert_eq!(value(keys::DEPTH), "0.9");
        assert_eq!(value(keys::DIAMETER), "0.15");
        assert_eq!(value(keys::OWNER), "WaterCo");
        assert_eq!(
            value(&format!("{}AssetFeature", keys::FIELD_PREFIX)),
            "Main"
        );
        assert_eq!(value(keys::HORIZONTAL_UNCERTAINTY), "0.2");
    }

    #[test]
    fn fields_that_name_no_column_are_refused_by_name() {
        let mut session = Session::default();
        session.ok("LAYER NEW gis");
        string(
            &mut session,
            "gis",
            vec![p(0.0, 0.0), p(8.0, 0.0)],
            &[],
            PropertyMap::new(),
            false,
        );
        let steps = session.steps();
        let mut refusal = |fields: &str| {
            session
                .refused(&format!(
                    "UTILITY DRAW LAYERS gis METHOD records FIELDS {fields}"
                ))
                .describe()
        };
        assert!(contains(
            &refusal("colour=COL"),
            "FIELDS: colour is not a column of a utility schedule"
        ));
        assert!(contains(
            &refusal("easting=X"),
            "FIELDS: easting is where the geometry is"
        ));
        assert!(contains(
            &refusal("Size=SZ"),
            "FIELDS: size is read from a schedule only"
        ));
        assert!(contains(
            &refusal("type"),
            "FIELDS needs column=property pairs"
        ));
        assert!(contains(
            &refusal("type=A,utility_type=B"),
            "FIELDS: type given twice"
        ));
        assert!(contains(
            &refusal("type="),
            "FIELDS: type names no property"
        ));
        assert_eq!(session.steps(), steps);
    }
}
