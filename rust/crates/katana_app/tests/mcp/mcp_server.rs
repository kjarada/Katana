//! tests/app/test_mcp_server.cpp: the MCP server (katana_app::mcp::server,
//! docs/mcp.md), driven message by message as a client would drive it,
//! without a process. What a command prints is the command line's own, which
//! the cli.* tests pin; these pin the protocol around it and what the tools
//! add: the batch that stops at a failure, the refusal to discard unsaved
//! work, and the state reported after each call.
//!
//! The six cases that set an environment variable, change the working
//! directory or read the real streams are in tests/mcp_process. Many cases
//! here run lines whose verbs are step 6's wave 2 (IMPORT, EXPORT, INFO,
//! REFS, SURFACE, CONTOUR and the raster verbs): they pass when wave 2 is
//! integrated as one.

mod mcp_server {
    use std::collections::BTreeMap;

    use katana_app::Session;
    use katana_app::mcp::LATEST_PROTOCOL_VERSION;
    use katana_core::json::Json;
    use katana_entity::entity::Color;

    use crate::app_support::customisation_file;
    use crate::geo_fixture::{
        TempDir, checkout, geo_data, gis_samples, holds, lacks, near, shared_data,
    };
    use crate::mcp_client::{
        Client, SURVEYIO_DATA, double_eq, double_of, integers, is_error, object, reals, string_of,
        strings, text_of,
    };

    // A directory of its own per test, removed afterwards (the C++'s TempDir).
    fn temp(name: &str) -> TempDir {
        TempDir::new("katana-mcp-", name)
    }

    // One of the committed Katana customisation files (tests/data/customisation),
    // its path quoted for a line.
    fn quoted_customisation(name: &str) -> String {
        format!("\"{}\"", customisation_file(name))
    }

    // The three of them in one load, as an agent loads a customisation: by
    // their text, 3 linestyles, 4 symbols, and 11 rules over 8 distinct keys.
    // The session had no name, so it takes the first one's, test_linestyles.
    #[track_caller]
    fn load_the_three_customisation_files(client: &mut Client) {
        let loaded = client.run_line(&format!(
            "CUSTOMISE {} {} {}",
            quoted_customisation("test_linestyles.customisation.json"),
            quoted_customisation("test_symbols.customisation.json"),
            quoted_customisation("test_survey.customisation.json")
        ));
        assert!(!is_error(&loaded), "{}", text_of(&loaded));
    }

    fn tools_of(client: &mut Client) -> Json {
        client.request("tools/list", Json::object())["result"]["tools"].clone()
    }

    // The tool of that name in a tools/list, or null.
    fn named(tools: &Json, name: &str) -> Json {
        tools
            .iter()
            .find(|tool| tool["name"] == Json::from(name))
            .cloned()
            .unwrap_or(Json::Null)
    }

    // Whether `at` is in `list` as a whole coordinate, not the start of a
    // longer one: the line ends after it.
    fn listed(list: &str, at: &str) -> bool {
        list.find(at).is_some_and(|found| {
            let end = found + at.len();
            end == list.len() || list.as_bytes()[end] == b'\n' || list.as_bytes()[end] == b'\r'
        })
    }

    // A file of samples/gis, as KATANA_GIS_SAMPLES names it.
    fn sample(name: &str) -> String {
        format!("{}/{name}", gis_samples())
    }

    // tests/geo/data/lots.geojson.
    fn lots() -> String {
        format!("{}/lots.geojson", geo_data())
    }

    fn output_of(line: &Json) -> String {
        string_of(&line["output"])
    }

    #[test]
    fn initialize_agrees_the_clients_version_and_names_the_server() {
        let mut client = Client::new();
        let reply = client.request(
            "initialize",
            object([
                ("protocolVersion", Json::from("2025-03-26")),
                ("capabilities", Json::object()),
            ]),
        );
        assert!(reply.contains("result"), "{}", reply.dump());
        assert_eq!(reply["result"]["protocolVersion"], Json::from("2025-03-26"));
        assert_eq!(reply["result"]["serverInfo"]["name"], Json::from("katana"));
        assert_eq!(
            reply["result"]["serverInfo"]["version"],
            Json::from("9.9.9")
        );
        assert!(reply["result"]["capabilities"].contains("tools"));
        assert!(reply["result"].contains("instructions"));
    }

    #[test]
    fn an_unknown_protocol_version_is_answered_with_the_latest() {
        let mut client = Client::new();
        let reply = client.request(
            "initialize",
            object([("protocolVersion", Json::from("1999-01-01"))]),
        );
        assert_eq!(
            reply["result"]["protocolVersion"],
            Json::from(LATEST_PROTOCOL_VERSION)
        );
    }

    #[test]
    fn notifications_are_never_answered() {
        let mut client = Client::new();
        assert!(
            client
                .server
                .handle(
                    &mut client.session,
                    r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#
                )
                .is_none()
        );
    }

    #[test]
    fn text_that_is_not_json_is_a_parse_error() {
        let mut client = Client::new();
        let reply = client
            .server
            .handle(&mut client.session, "not json")
            .expect("a reply");
        let error = Json::parse(reply).expect("JSON");
        assert_eq!(error["error"]["code"], Json::from(-32700));
        assert!(error["id"].is_null());
    }

    #[test]
    fn an_unknown_method_is_method_not_found() {
        let mut client = Client::new();
        assert_eq!(
            client.request("no/such/method", Json::object())["error"]["code"],
            Json::from(-32601)
        );
    }

    #[test]
    fn ping_is_answered_with_an_empty_result() {
        let mut client = Client::new();
        assert_eq!(
            client.request("ping", Json::object())["result"],
            Json::object()
        );
    }

    #[test]
    fn every_tool_is_listed_with_an_object_schema() {
        let mut client = Client::new();
        client.initialize();
        let tools = tools_of(&mut client);
        assert!(tools.is_array());
        let mut names = Vec::new();
        for tool in tools.iter() {
            names.push(string_of(&tool["name"]));
            assert_eq!(
                tool["inputSchema"]["type"],
                Json::from("object"),
                "{}",
                tool["name"].dump()
            );
            assert!(
                !string_of(&tool["description"]).is_empty(),
                "{}",
                tool["name"].dump()
            );
        }
        for expected in [
            "katana_run_commands",
            "katana_run_script",
            "katana_help",
            "katana_status",
            "katana_new_project",
            "katana_open_project",
            "katana_save_project",
            "katana_list_entities",
            "katana_describe_entity",
            "katana_import",
            "katana_export",
            "katana_undo",
        ] {
            assert!(names.iter().any(|name| name == expected), "{expected}");
        }
    }

    // What an agent reads first names every UTILITY action and the scope words
    // they take, so what is drawn can be graded again and written back - and a
    // survey or an import drawn as services - without asking HELP first.
    #[test]
    fn the_command_tool_names_every_utility_action_and_the_scope_words() {
        let mut client = Client::new();
        client.initialize();
        let tools = tools_of(&mut client);
        let description = string_of(&named(&tools, "katana_run_commands")["description"]);
        assert!(!description.is_empty());
        for word in [
            "UTILITY REPORT",
            "VERIFY",
            "CLEARANCE",
            "CHECK",
            "DRAW",
            "REGRADE",
            "SCHEDULE",
            "HELP UTILITY",
            "DRAWING",
            "SELECTION",
            "AREA x0,y0,x1,y1",
            "LAYERS a,b [ONLY]",
            "WHERE key=value",
            "MODIFY",
            "UTILITY DRAW <scope> METHOD <method>",
            "FIELDS column=property",
        ] {
            assert!(description.contains(word), "{word}");
        }
    }

    // And the survey field-file verbs, with the way to hold a point of the
    // drawing, so an agent finds how to import a file with no coordinates from
    // the tool list alone.
    #[test]
    fn the_command_tool_names_the_survey_field_file_verbs() {
        let mut client = Client::new();
        client.initialize();
        let tools = tools_of(&mut client);
        let description = string_of(&named(&tools, "katana_run_commands")["description"]);
        assert!(!description.is_empty());
        for words in [
            "SURVEY READ <file>",
            "SURVEY IMPORT <file>",
            "SETTINGS <file>",
            "SET key=value",
            "SET control=<id>;drawing;",
            "FORWARD",
        ] {
            assert!(description.contains(words), "{words}");
        }
        // What an import does with the survey codes, and the verb that strings
        // coded points afterwards: an agent that is not told of the two words
        // cannot turn the coding off, and one not told of LINEWORK draws the
        // lines by hand. The verb's own words as HELP LINEWORK lists them,
        // with PROPERTY and CHORD since the verb took them.
        for words in [
            "[CODES on|off] [LINEWORK on|off]",
            "LINEWORK [<scope>]",
            "[PROPERTY <name>] [ORDER number|entity] [CHORD <length>] [PREVIEW]",
            "HELP LINEWORK",
        ] {
            assert!(description.contains(words), "{words}");
        }
    }

    #[test]
    fn an_unknown_tool_is_invalid_params() {
        let mut client = Client::new();
        assert_eq!(
            client.request(
                "tools/call",
                object([("name", Json::from("katana_nothing"))])
            )["error"]["code"],
            Json::from(-32602)
        );
    }

    #[test]
    fn commands_draw_into_the_live_drawing_and_report_its_state() {
        let mut client = Client::new();
        client.initialize();
        let result = client.run_lines(&[
            "LAYER NEW Kerb #FF0000",
            "LAYER SET Kerb",
            "RECT 0,0 30,20",
            "CIRCLE 15,10 5",
        ]);
        assert!(!is_error(&result), "{}", text_of(&result));
        holds(&text_of(&result), "> RECT 0,0 30,20\nrectangle created");
        let status = &result["structuredContent"]["status"];
        assert_eq!(status["entities"], Json::from(2));
        assert_eq!(status["currentLayer"], Json::from("Kerb"));
        assert_eq!(status["modified"], Json::from(true));

        let list = client.call_bare("katana_list_entities");
        holds(
            &text_of(&list),
            "Polyline  layer=Kerb  vertices=4  closed  length=100  area=600",
        );
    }

    #[test]
    fn a_batch_stops_at_the_first_failing_command_and_says_which() {
        let mut client = Client::new();
        client.initialize();
        let result = client.run_lines(&["CIRCLE 0,0 -1", "POINT 1,1"]);
        assert!(is_error(&result));
        let lines = &result["structuredContent"]["commands"];
        assert_eq!(lines.size(), 2);
        assert_eq!(lines[0]["ok"], Json::from(false));
        holds(&string_of(&lines[0]["messages"]), "radius must be positive");
        assert_eq!(lines[1]["skipped"], Json::from(true));
        assert_eq!(
            result["structuredContent"]["status"]["entities"],
            Json::from(0)
        );
    }

    // What an agent is told of a field file whose setup stands on RTK marks,
    // and where the import puts the shots: the hand-built file of the surveyio
    // tests, its positions worked by hand in src/katana_app/CMakeLists.txt
    // beside cli.survey_import_rtk_setup_field_file - the marks, two GNSS
    // positions, orient the setup, and each shot's offset is applied.
    #[test]
    fn an_agent_imports_a_field_file_whose_setup_stands_on_rtk_marks() {
        let mut client = Client::new();
        client.initialize();
        let file = format!("{SURVEYIO_DATA}/fld/rtk_setup.fld");
        let result = client.run_lines(&[
            &format!("SURVEY READ \"{file}\""),
            &format!("SURVEY IMPORT \"{file}\""),
            "LIST",
        ]);
        assert!(!is_error(&result), "{}", text_of(&result));
        let lines = &result["structuredContent"]["commands"];
        assert_eq!(lines.size(), 3);
        let read = output_of(&lines[0]);
        holds(
            &read,
            "survey file=rtk_setup.fld format=opcode-field-file parser=1.2 read=18 skipped=0 \
             warnings=0",
        );
        // Nine observations of the setup and the marks' two GNSS positions;
        // the marks and the shots are all placed by the reduction.
        holds(
            &read,
            "content setups=1 observations=11 points=0 unpositioned=4",
        );
        holds(
            &output_of(&lines[1]),
            "imported job=job-1 entities=4 layer=survey/points",
        );
        let list = output_of(&lines[2]);
        assert!(listed(&list, " at 1010.5,5000"), "{list}");
        assert!(listed(&list, " at 1020,4999.5"), "{list}");
        assert_eq!(
            result["structuredContent"]["status"]["entities"],
            Json::from(4)
        );
    }

    // An agent reaches a survey field file and a WKT through the lines it
    // sends: SURVEY IMPORT is the session's verb (survey_verbs), and CRS SET
    // takes the WKT with its quotes, which the verb once lost. The field file
    // is the hand-built one of the surveyio tests; its 4 points are worked by
    // hand in src/katana_app/CMakeLists.txt, beside cli.survey_import_field_file.
    #[test]
    fn an_agent_imports_a_field_file_and_sets_the_system_by_its_wkt() {
        let mut client = Client::new();
        client.initialize();
        let file = format!("{SURVEYIO_DATA}/fld/setup.fld");
        // OGC WKT1 for EPSG:7856, a name with blanks in it: without its quotes
        // PROJ cannot read it.
        let wkt = concat!(
            "PROJCS[\"GDA2020 / MGA zone 56\",GEOGCS[\"GDA2020\",DATUM[\"Geocentric_Datum_of_",
            "Australia_2020\",SPHEROID[\"GRS 1980\",6378137,298.257222101]],PRIMEM[\"Greenwich\",0],",
            "UNIT[\"degree\",0.0174532925199433]],PROJECTION[\"Transverse_Mercator\"],",
            "PARAMETER[\"latitude_of_origin\",0],PARAMETER[\"central_meridian\",153],",
            "PARAMETER[\"scale_factor\",0.9996],PARAMETER[\"false_easting\",500000],",
            "PARAMETER[\"false_northing\",10000000],UNIT[\"metre\",1],AUTHORITY[\"EPSG\",\"7856\"]]"
        );
        let result = client.run_lines(&[
            &format!("SURVEY IMPORT \"{file}\""),
            &format!("CRS SET {wkt}"),
        ]);
        assert!(!is_error(&result), "{}", text_of(&result));
        let lines = &result["structuredContent"]["commands"];
        assert_eq!(lines.size(), 2);
        let imported = output_of(&lines[0]);
        holds(&imported, "survey file=setup.fld format=opcode-field-file");
        holds(
            &imported,
            "imported job=job-1 entities=4 layer=survey/points",
        );
        assert_eq!(
            output_of(&lines[1]),
            "crs id=EPSG:7856 name=\"GDA2020 / MGA zone 56\" kind=\"projected\" units=metre"
        );
        assert_eq!(
            result["structuredContent"]["status"]["entities"],
            Json::from(4)
        );
    }

    // A Sokkia SDR file reaches an agent through the same lines: the registry
    // gives SURVEY READ and SURVEY IMPORT every format surveyio reads. The file
    // is the hand-built one of the surveyio tests; its 4 points are worked by
    // hand in src/katana_app/CMakeLists.txt, beside cli.survey_import_sokkia_sdr,
    // and its 8 warnings are its 7 deleted records and its coordinate order
    // (option 2, read east first), said once.
    #[test]
    fn an_agent_reads_and_imports_a_sokkia_sdr_file() {
        let mut client = Client::new();
        client.initialize();
        let file = format!("{SURVEYIO_DATA}/sdr/traverse.sdr");
        let result = client.run_lines(&[
            &format!("SURVEY READ \"{file}\""),
            &format!("SURVEY IMPORT \"{file}\""),
        ]);
        assert!(!is_error(&result), "{}", text_of(&result));
        let lines = &result["structuredContent"]["commands"];
        assert_eq!(lines.size(), 2);
        let read = output_of(&lines[0]);
        holds(
            &read,
            "survey file=traverse.sdr format=sokkia-sdr parser=1.0 read=40 skipped=7 warnings=8",
        );
        holds(
            &read,
            "warning record=8 text=\"the header's coordinate order option is 2",
        );
        holds(
            &read,
            "warning record=35 text=\"a deleted record (marked 'DD'), 02NM (station), is not \
             imported\"",
        );
        holds(
            &read,
            "content setups=2 observations=36 points=2 unpositioned=2",
        );
        holds(
            &output_of(&lines[1]),
            "imported job=job-1 entities=4 layer=survey/points",
        );
        assert_eq!(
            result["structuredContent"]["status"]["entities"],
            Json::from(4)
        );
    }

    // An agent imports a field file that gives no coordinates by holding a
    // point of the drawing, as the wizard's control can: FORWARD puts CP1
    // there, and SURVEY IMPORT's SET holds it (survey_verbs). The file is
    // traverse.sdr less its coordinate records; alone it places nothing. Held,
    // it places CP2, T1 and T2 where src/katana_app/CMakeLists.txt works them
    // by hand beside cli.survey_import_holds_a_point_of_the_drawing, and CP1
    // stays the drawing's, not drawn again.
    #[test]
    fn an_agent_imports_a_file_with_no_coordinates_holding_a_point_of_the_drawing() {
        let mut client = Client::new();
        client.initialize();
        let file = format!("{SURVEYIO_DATA}/sdr/traverse_without_coordinates.sdr");
        let result = client.run_lines(&[
            &format!("SURVEY IMPORT \"{file}\""),
            "FORWARD 500000,4999999,100 0 1 0 CP1",
            &format!("SURVEY IMPORT \"{file}\" SET control=CP1;drawing;fixed;0;fixed;0;fixed;0"),
            "LIST",
        ]);
        assert!(!is_error(&result), "{}", text_of(&result));
        let lines = &result["structuredContent"]["commands"];
        assert_eq!(lines.size(), 4);
        holds(
            &output_of(&lines[0]),
            "imported job=job-1 entities=0 layer=survey/points",
        );
        let held = output_of(&lines[2]);
        holds(
            &held,
            "\nsettings file= set=1 differ=1\nsetting key=control \
             value=CP1;drawing;fixed;0;fixed;0;fixed;0\nimported job=job-3 entities=3 \
             layer=survey/points",
        );
        // Where CP1 was held: the drawing's point FORWARD made, entity 2 - the
        // first import's job took number 1 (a job is numbered with the next
        // entity number) - and nothing adjusted, as the defaults have it.
        holds(
            &held,
            "\nheld id=CP1 from=drawing entity=2 northing=5e+06 easting=5e+05 height=100\n\
             reduction method=radiation adjustments=0 rejected=0\n",
        );
        let list = output_of(&lines[3]);
        assert!(listed(&list, " at 500000,5000000"), "{list}");
        assert!(listed(&list, " at 500000,5000099.99993"), "{list}");
        assert!(listed(&list, " at 500149.795756,5000000"), "{list}");
        assert!(listed(&list, " at 500149.795756,5000079.98883"), "{list}");
        assert_eq!(
            result["structuredContent"]["status"]["entities"],
            Json::from(4)
        );
    }

    // HELP sent as a command is the session's whole help, what katana_help
    // and --help give: IFC is the session's, not the interpreter's, and the
    // interpreter's own HELP leaves it out.
    #[test]
    fn help_sent_as_a_command_is_the_whole_session_help() {
        let mut client = Client::new();
        client.initialize();
        let result = client.run_lines(&["HELP", "?"]);
        assert!(!is_error(&result), "{}", text_of(&result));
        let text = text_of(&result);
        // The reply's text is trimmed at its end, so the help is sought
        // without its closing newline.
        let mut help = Session::help_text();
        while help.ends_with('\n') {
            help.pop();
        }
        let first = text.find(&help).unwrap_or_else(|| panic!("{text}"));
        assert!(text[first + help.len()..].contains(&help), "{text}");
        holds(&text, "CUSTOMISE [REPLACE]");
        holds(&text, "IMPORT <file.ifc>");
    }

    #[test]
    fn a_batch_told_to_carry_on_runs_past_a_failure() {
        let mut client = Client::new();
        client.initialize();
        let result = client.call(
            "katana_run_commands",
            object([
                ("commands", strings(&["CIRCLE 0,0 -1", "POINT 1,1"])),
                ("stop_on_error", Json::from(false)),
            ]),
        );
        assert!(is_error(&result));
        assert_eq!(
            result["structuredContent"]["status"]["entities"],
            Json::from(1)
        );
    }

    #[test]
    fn new_and_open_refuse_to_discard_unsaved_changes_unless_told_to() {
        let mut client = Client::new();
        client.initialize();
        let _ = client.run_lines(&["POINT 1,1"]);

        let refused = client.call_bare("katana_new_project");
        assert!(is_error(&refused));
        holds(&text_of(&refused), "unsaved changes");
        assert_eq!(
            client.call_bare("katana_status")["structuredContent"]["entities"],
            Json::from(1)
        );

        // A typed NEW is held to the same rule as the tool.
        assert!(is_error(&client.run_lines(&["NEW"])));

        let done = client.call(
            "katana_new_project",
            object([("discard_unsaved_changes", Json::from(true))]),
        );
        assert!(!is_error(&done), "{}", text_of(&done));
        assert_eq!(
            client.call_bare("katana_status")["structuredContent"]["entities"],
            Json::from(0)
        );
    }

    #[test]
    fn opening_a_polyline_is_an_edit_not_a_discard_of_the_drawing() {
        // OPEN SELECTION (and OPEN #id) opens polylines (docs/drawing.md): it
        // keeps the drawing, so the unsaved-changes guard NEW and OPEN of a
        // project are held to does not refuse it.
        let mut client = Client::new();
        client.initialize();
        let result = client.run_lines(&["RECT 0,0 30,20", "SELECT ALL", "OPEN SELECTION"]);
        assert!(!is_error(&result), "{}", text_of(&result));
        assert_eq!(
            result["structuredContent"]["status"]["entities"],
            Json::from(1)
        );
    }

    #[test]
    fn a_project_saved_by_one_session_opens_in_another() {
        let dir = temp("save-open");
        let project = dir.file("site project");
        let mut client = Client::new();
        client.initialize();
        let _ = client.run_lines(&["RECT 0,0 30,20"]);
        let saved = client.call(
            "katana_save_project",
            object([("path", Json::from(&project))]),
        );
        assert!(!is_error(&saved), "{}", text_of(&saved));
        assert_eq!(
            saved["structuredContent"]["status"]["modified"],
            Json::from(false)
        );

        // Another session and its own server, which nobody initialized.
        let mut other = Client::over(Session::new(None, &mut katana_app::Captured::default()));
        let opened = other.call(
            "katana_open_project",
            object([("path", Json::from(&project))]),
        );
        assert!(!is_error(&opened), "{}", opened.dump());
        assert_eq!(other.session.document().model().entities.size(), 1);
    }

    #[test]
    fn a_path_with_a_quote_is_refused_rather_than_cut_short() {
        let mut client = Client::new();
        client.initialize();
        let result = client.call(
            "katana_save_project",
            object([("path", Json::from("a\"b"))]),
        );
        assert!(is_error(&result));
        assert!(!client.session.document().has_project());
    }

    #[test]
    fn a_command_with_a_line_break_is_refused() {
        let mut client = Client::new();
        client.initialize();
        let result = client.run_lines(&["POINT 1,1\nERASE"]);
        assert!(is_error(&result));
        assert_eq!(client.session.document().model().entities.size(), 0);
    }

    #[test]
    fn quit_is_not_run_and_the_session_carries_on() {
        let mut client = Client::new();
        client.initialize();
        let result = client.run_lines(&["QUIT", "POINT 1,1"]);
        assert!(!is_error(&result));
        assert_eq!(client.session.document().model().entities.size(), 1);
    }

    #[test]
    fn describe_entity_describes_the_entity_it_names() {
        // The tool sends INFO #<id>, which a session with the GIS module once
        // took for INFO <file>: every call answered that the file did not
        // exist. A 10 x 5 rectangle: perimeter 30, area 50.
        let mut client = Client::new();
        client.initialize();
        let _ = client.run_lines(&["RECT 0,0 10,5"]);
        let described = client.call("katana_describe_entity", object([("id", Json::from(1))]));
        assert!(!is_error(&described), "{}", text_of(&described));
        holds(
            &text_of(&described),
            "> INFO #1\n1  Polyline  layer=0  vertices=4  closed  length=30  area=50",
        );

        let missing = client.call("katana_describe_entity", object([("id", Json::from(2))]));
        assert!(is_error(&missing));
        holds(&text_of(&missing), "entity does not exist");
    }

    // ---- leaders (docs/mcp.md, "Leaders") ------------------------------------------------------
    //
    // katana_leaders: what each leader says now, and the presets, read from
    // the Document as katana_status is - LEADER LIST JSON's object. A point at
    // RL 25 with a level leader on it says "RL 25.000".

    #[test]
    fn leader_points_and_frames_are_editable_and_their_full_geometry_is_reported() {
        let mut client = Client::new();
        client.initialize();
        let result = client.run_lines(&[
            "LEADER 0,0 5,5 10,5 text=Note",
            "LEADER SET 1 vertex=1 at=7,8 callout=box landingangle=180 paper=3",
        ]);
        assert!(!is_error(&result), "{}", text_of(&result));
        let report = client.call("katana_leaders", object([("scope", Json::from("drawing"))]));
        assert!(!is_error(&report), "{}", text_of(&report));
        let leaders = &report["structuredContent"]["leaders"];
        assert_eq!(leaders.size(), 1);
        assert_eq!(
            leaders[0]["points"],
            Json::array_of([reals(&[0.0, 0.0]), reals(&[7.0, 8.0]), reals(&[10.0, 5.0])])
        );
        assert_eq!(leaders[0]["callout"], Json::from("box"));
        assert_eq!(leaders[0]["paper_height"], Json::from(3));
        assert!(leaders[0]["landing_angle"].is_number());
        let undone = client.call_bare("katana_undo");
        assert!(!is_error(&undone), "{}", text_of(&undone));
        let original = client.call("katana_leaders", object([("scope", Json::from("drawing"))]));
        assert_eq!(
            original["structuredContent"]["leaders"][0]["points"],
            Json::array_of([reals(&[0.0, 0.0]), reals(&[5.0, 5.0]), reals(&[10.0, 5.0])])
        );
        assert_eq!(
            original["structuredContent"]["leaders"][0]["callout"],
            Json::from("none")
        );
    }

    #[test]
    fn the_leaders_tool_gives_what_each_leader_says_and_the_presets() {
        let mut client = Client::new();
        client.initialize();
        let made = client.run_lines(&[
            "POINT 30,40",
            "SELECT 1",
            "PROP SET elevation 25",
            "LEADER #1 40,50 preset=level",
        ]);
        assert!(!is_error(&made), "{}", text_of(&made));
        let steps = client.session.document().history().undo_count();

        let all = client.call_bare("katana_leaders");
        assert!(!is_error(&all), "{}", text_of(&all));
        let leaders = &all["structuredContent"]["leaders"];
        assert_eq!(leaders.size(), 1, "{}", all.dump());
        assert_eq!(leaders[0]["id"], Json::from(2));
        assert_eq!(leaders[0]["target"], Json::from(1));
        assert_eq!(leaders[0]["preset"], Json::from("level"));
        assert!(leaders[0]["reads"].is_null());
        assert_eq!(leaders[0]["template"], Json::from("RL {rl:.3f}"));
        assert_eq!(leaders[0]["text"], Json::from("RL 25.000"));
        assert_eq!(leaders[0]["lines"], strings(&["RL 25.000"]));
        let presets = &all["structuredContent"]["presets"];
        assert_eq!(presets.size(), 7);
        assert_eq!(presets[1]["name"], Json::from("surface"));
        assert_eq!(presets[1]["reads"], Json::from("surface"));
        assert_eq!(presets[1]["tool"], Json::from("annotate.leader.surface"));
        assert!(all["structuredContent"]["surfaces"].empty());
        assert_eq!(
            client.session.document().history().undo_count(),
            steps,
            "a read is no step"
        );

        // Only the leaders on an entity, or of given ids.
        assert_eq!(
            client.call("katana_leaders", object([("target", Json::from(1))]))["structuredContent"]
                ["leaders"]
                .size(),
            1
        );
        assert!(
            client.call("katana_leaders", object([("ids", integers(&[99]))]))["structuredContent"]
                ["leaders"]
                .empty()
        );
        assert!(is_error(
            &client.call("katana_leaders", object([("target", Json::from(0))]))
        ));
        assert!(is_error(
            &client.call("katana_leaders", object([("ids", strings(&["two"]))]))
        ));
    }

    // katana_leaders takes the one scope and filter grammar as parameters
    // (CLAUDE.md 1.1), reads the leaders the scope took, and says how many. By
    // hand: points 1 (0,0) and 2 (50,50); on layer notes the leaders 3
    // (0,0)-(5,5) and 4 (50,50)-(55,55); on layer 0 the leader 5
    // (100,100)-(110,110).
    #[test]
    fn the_leaders_tool_takes_a_scope_and_filter_and_says_how_many_leaders_it_took() {
        let mut client = Client::new();
        client.initialize();
        let made = client.run_lines(&[
            "LAYER NEW notes",
            "POINT 0,0",
            "POINT 50,50",
            "LAYER SET notes",
            "LEADER 0,0 5,5 text=A",
            "LEADER 50,50 55,55 text=B",
            "LAYER SET 0",
            "LEADER 100,100 110,110 text=C",
        ]);
        assert!(!is_error(&made), "{}", text_of(&made));
        let steps = client.session.document().history().undo_count();
        let ids_of = |reply: &Json| -> Vec<i64> {
            reply["structuredContent"]["leaders"]
                .iter()
                .map(|leader| leader["id"].get::<i64>().expect("an id"))
                .collect()
        };

        // A layer: the two on notes, and the scope says so.
        let layers = client.call(
            "katana_leaders",
            object([
                ("scope", Json::from("layers")),
                ("scope_layers", strings(&["notes"])),
            ]),
        );
        assert!(!is_error(&layers), "{}", text_of(&layers));
        assert_eq!(ids_of(&layers), vec![3, 4]);
        assert_eq!(
            layers["structuredContent"]["scope"]["matched"],
            Json::from(2)
        );
        assert_eq!(
            layers["structuredContent"]["scope"]["record"],
            Json::from("scope=layers layers=notes sublayers=yes matched=2")
        );

        // An area given alone is the scope: -1,-1,10,10 overlaps only leader 3.
        let area = client.call(
            "katana_leaders",
            object([("area", integers(&[-1, -1, 10, 10]))]),
        );
        assert!(!is_error(&area), "{}", text_of(&area));
        assert_eq!(ids_of(&area), vec![3]);
        assert_eq!(
            area["structuredContent"]["scope"]["record"],
            Json::from("scope=area area=-1,-1,10,10 matched=1")
        );

        // The whole drawing, with a filter: the leaders on layer 0 are leader 5.
        let filtered = client.call(
            "katana_leaders",
            object([
                ("scope", Json::from("drawing")),
                ("scope_where", strings(&["LAYER=0"])),
            ]),
        );
        assert_eq!(ids_of(&filtered), vec![5]);

        // A scope that takes no leader gives none - not all of them.
        let none = client.call(
            "katana_leaders",
            object([
                ("scope", Json::from("drawing")),
                ("scope_where", strings(&["TYPE=point"])),
            ]),
        );
        assert!(!is_error(&none), "{}", text_of(&none));
        assert!(
            none["structuredContent"]["leaders"].empty(),
            "{}",
            none.dump()
        );
        assert_eq!(none["structuredContent"]["scope"]["matched"], Json::from(0));
        // A filter alone filters the selection, which is empty.
        let selection = client.call(
            "katana_leaders",
            object([("scope_where", strings(&["TYPE=point"]))]),
        );
        assert!(selection["structuredContent"]["leaders"].empty());
        assert_eq!(
            selection["structuredContent"]["scope"]["matched"],
            Json::from(0)
        );

        // No scope: all three, and no scope member.
        let all = client.call_bare("katana_leaders");
        assert_eq!(ids_of(&all), vec![3, 4, 5]);
        assert!(!all["structuredContent"].contains("scope"));
        // target narrows a scope too: none of these three is on an entity.
        assert!(
            client.call(
                "katana_leaders",
                object([("scope", Json::from("drawing")), ("target", Json::from(1))])
            )["structuredContent"]["leaders"]
                .empty()
        );

        // The refusals: ids and a scope; a layer that is not there; VIEW,
        // which is the window's; an area that is not four numbers.
        assert!(is_error(&client.call(
            "katana_leaders",
            object([("ids", integers(&[3])), ("scope", Json::from("drawing"))])
        )));
        assert!(is_error(&client.call(
            "katana_leaders",
            object([
                ("scope", Json::from("layers")),
                ("scope_layers", strings(&["nowhere"])),
            ])
        )));
        assert!(is_error(&client.call(
            "katana_leaders",
            object([("scope", Json::from("view"))])
        )));
        assert!(is_error(&client.call(
            "katana_leaders",
            object([("scope", Json::from("area")), ("area", integers(&[1, 2]))])
        )));
        assert_eq!(
            client.session.document().history().undo_count(),
            steps,
            "a read is no step"
        );
    }

    #[test]
    fn a_scripts_indented_note_runs_nothing() {
        // A line whose first non-blank is '#' is a note, as File > Run Script
        // reads one; an indented one was once a command here, so the script
        // stopped at it and the lines after it were "not run".
        let folder = temp("indented-notes");
        std::fs::write(
            folder.file("notes.kcs"),
            "# a note\r\nRECT 0,0 10,5\r\n\r\n  # an indented note\r\n\t# a tabbed one\r\n\
             CIRCLE 5,5 1\r\nLIST\r\n",
        )
        .expect("the script is written");
        let mut client = Client::new();
        client.initialize();
        let ran = client.call(
            "katana_run_script",
            object([("path", Json::from(folder.file("notes.kcs")))]),
        );
        assert!(!is_error(&ran), "{}", text_of(&ran));
        assert_eq!(
            client.session.document().model().entities.size(),
            2,
            "{}",
            text_of(&ran)
        );
        lacks(&text_of(&ran), "not run");
    }

    #[test]
    fn a_script_saved_in_the_ansi_code_page_is_read_as_text_and_its_names_are_utf8() {
        // A script saved from an ANSI editor on Windows holds e-acute as the
        // byte 0xE9 (Windows-1252, CP1252.TXT), which is no UTF-8.
        // katana_run_script read it as bytes and the name went to the model as
        // they were: LAYER NEW refused it, and a surface or style named so
        // ended the server's next SAVE or JSON reply. Decoded as a text file
        // is (core's decode_text) it is "Café" - C3 A9 - and the guess is said
        // on stderr, the server's log.
        let folder = temp("ansi-script");
        std::fs::write(folder.file("ansi.kcs"), b"LAYER NEW Caf\xE9\nLAYER LIST\n")
            .expect("the script is written");
        // A UTF-8 script with a byte order mark and Windows line ends, as
        // Notepad writes "UTF-8 with BOM": the mark is not part of the first
        // word.
        std::fs::write(
            folder.file("bom.kcs"),
            b"\xEF\xBB\xBFLAYER NEW Pente\xC2\xB0\r\nLAYER LIST\r\n",
        )
        .expect("the script is written");
        let mut client = Client::new();
        client.initialize();
        let ansi = client.call(
            "katana_run_script",
            object([("path", Json::from(folder.file("ansi.kcs")))]),
        );
        assert!(!is_error(&ansi), "{}", text_of(&ansi));
        holds(&text_of(&ansi), "Caf\u{e9}");
        assert!(
            !text_of(&ansi)
                .as_bytes()
                .windows(4)
                .any(|four| four == b"Caf\xE9"),
            "no raw 0xE9 in the reply"
        );

        let bom = client.call(
            "katana_run_script",
            object([("path", Json::from(folder.file("bom.kcs")))]),
        );
        assert!(!is_error(&bom), "{}", text_of(&bom));
        holds(&text_of(&bom), "Pente\u{b0}");
        assert!(!text_of(&bom).contains('\u{feff}'), "the BOM is not a word");
        // A script that is not there says so, and names the path.
        let missing = client.call(
            "katana_run_script",
            object([("path", Json::from(folder.file("none.kcs")))]),
        );
        assert!(is_error(&missing));
        holds(&text_of(&missing), "cannot read the script");
    }

    #[test]
    fn the_windows_own_verbs_are_refused_saying_where_they_run() {
        // PLOT, SNAPSHOT, ONLINE and SCRIPT run only in the desktop window; an
        // agent once met "unknown command" and could not tell a typo from a
        // verb this surface lacks. The tool's description says so too. GRID
        // and EXAGGERATION were unknown commands here until 2026-09-30; VIEWS
        // and ZOOM are refused by the interpreter itself, which has no views
        // here.
        let mut client = Client::new();
        client.initialize();
        for line in [
            "PLOT a.pdf",
            "PLOTSHEETS",
            "SNAPSHOT a.png",
            "ONLINE PROVIDERS",
            "SCRIPT a.kcs",
            "GRID ON",
            "EXAGGERATION 2",
            "VIEWS",
            "VIEWS LINK 1,2",
            "ZOOM IN",
            "Z",
        ] {
            let ran = client.run_lines(&[line]);
            assert!(is_error(&ran), "{line}");
            holds(&text_of(&ran), "is the desktop window's");
            lacks(&text_of(&ran), "unknown command");
        }
        let tools = tools_of(&mut client);
        let run = named(&tools, "katana_run_commands");
        assert!(run.is_object());
        holds(&string_of(&run["description"]), "desktop window's verbs");
    }

    // The update system's one tool (docs/mcp.md, "Updates"): it asks and
    // reports, and says to its client that it reaches the network. The tools
    // that run lines do not, and go on saying so, because an UPDATE line is
    // refused in them.
    #[test]
    fn the_update_tool_is_listed_read_only_and_open_world_and_the_tools_that_run_lines_are_not() {
        let mut client = Client::new();
        client.initialize();
        let tools = tools_of(&mut client);
        let update = named(&tools, "katana_check_update");
        assert!(update.is_object(), "katana_check_update is not listed");
        // Read-only and open-world, and nothing else: the WHOLE object. A tool
        // that is not read-only is listed with two hints more - whether it
        // destroys, and whether asking twice is asking once (hints) - which
        // mean something only for a tool that changes things; so their absence
        // is the tool being listed as one that changes nothing, and a tool
        // listed any other way fails here.
        assert_eq!(
            update["annotations"],
            object([
                ("readOnlyHint", Json::from(true)),
                ("openWorldHint", Json::from(true)),
            ]),
            "{}",
            update["annotations"].dump()
        );
        assert!(
            update["inputSchema"]["properties"].empty(),
            "it takes no argument"
        );
        let description = string_of(&update["description"]);
        // Among them what is sent, the one case in which a token is included:
        // a client is told the whole of it before it lets the tool run.
        for words in [
            "Read-only",
            "downloads nothing",
            "It reaches the network",
            "the program's version and platform",
            "KATANA_UPDATE_TOKEN",
            "api.github.com, and to no other host",
            "update=unsupported reason=development_tree",
            "never the token",
        ] {
            assert!(description.contains(words), "{words}");
        }

        for runs_lines in ["katana_run_commands", "katana_run_script"] {
            let tool = named(&tools, runs_lines);
            assert!(tool.is_object(), "{runs_lines}");
            assert_eq!(
                tool["annotations"]["openWorldHint"],
                Json::from(false),
                "{runs_lines}"
            );
        }
        let commands_description = string_of(&named(&tools, "katana_run_commands")["description"]);
        holds(&commands_description, "UPDATE lines are refused here too");
        holds(&commands_description, "katana_check_update");
    }

    #[test]
    fn an_update_line_is_refused_in_every_tool_that_runs_lines_naming_the_update_tool() {
        let mut client = Client::new();
        client.initialize();
        for line in [
            "UPDATE CHECK",
            "UPDATE STATUS",
            "update download confirm",
            "UPDATE APPLY CONFIRM",
            "UPDATE SKIP 1.0",
            "UPDATE BLOCKED CLEAR",
            "update blocked clear",
            "UPDATE SET AUTO OFF",
            "UPDATE TOKEN STATUS",
            "  UPDATE",
        ] {
            let ran = client.run_line(line);
            assert!(is_error(&ran), "{line}");
            let text = text_of(&ran);
            holds(&text, "Unsupported: UPDATE lines are not run by katana_mcp");
            holds(&text, "katana_check_update");
            lacks(&text, "unknown command");
            assert_eq!(
                ran["structuredContent"]["commands"][0]["ok"],
                Json::from(false),
                "{line}"
            );
        }

        // In a batch it stops the batch, as any refused line does: the point
        // before it is drawn, the one after it is not.
        let batch = client.run_lines(&["POINT 1,1", "UPDATE CHECK", "POINT 2,2"]);
        assert!(is_error(&batch));
        assert_eq!(
            client.session.document().model().entities.size(),
            1,
            "{}",
            text_of(&batch)
        );

        // And in a script, which the other tool that runs lines runs.
        let folder = temp("update-script");
        let script = folder.file("update.kcs");
        std::fs::write(&script, "POINT 3,3\nUPDATE DOWNLOAD CONFIRM\nPOINT 4,4\n")
            .expect("the script is written");
        let scripted = client.call("katana_run_script", object([("path", Json::from(&script))]));
        assert!(is_error(&scripted));
        holds(&text_of(&scripted), "katana_check_update");
        assert_eq!(
            client.session.document().model().entities.size(),
            2,
            "{}",
            text_of(&scripted)
        );

        // HELP UPDATE is a HELP line: the grammar is read here as anywhere.
        let help = client.run_line("HELP UPDATE");
        assert!(!is_error(&help), "{}", text_of(&help));
        holds(&text_of(&help), "UPDATE DOWNLOAD CONFIRM");
        holds(
            &text_of(&client.call_bare("katana_help")),
            "Update    UPDATE CHECK",
        );
    }

    #[test]
    fn the_import_tool_says_what_local_does() {
        // It once said LOCAL imported "in the drawing's own coordinates rather
        // than reprojecting": LOCAL moves the data, it does not keep it.
        let mut client = Client::new();
        client.initialize();
        let tools = tools_of(&mut client);
        let tool = named(&tools, "katana_import");
        assert!(tool.is_object());
        let local = string_of(&tool["inputSchema"]["properties"]["local"]["description"]);
        holds(&local, "lower-left corner sits at 0,0");
        lacks(&string_of(&tool["description"]), "reprojecting");
        let placement = &tool["inputSchema"]["properties"]["placement"];
        assert_eq!(
            placement["enum"],
            strings(&["keep", "local", "alongside", "offset"])
        );
    }

    #[test]
    fn the_import_tool_refuses_a_placement_it_cannot_say() {
        let mut client = Client::new();
        client.initialize();
        let mut refused = |arguments: Json| -> String {
            let result = client.call("katana_import", arguments.clone());
            assert!(is_error(&result), "{}", arguments.dump());
            text_of(&result)
        };
        holds(
            &refused(object([
                ("path", Json::from("a.dxf")),
                ("placement", Json::from("beside")),
            ])),
            "keep, local",
        );
        holds(
            &refused(object([
                ("path", Json::from("a.dxf")),
                ("placement", Json::from("offset")),
                ("offset_east", Json::from(1)),
            ])),
            "offset_north",
        );
        holds(
            &refused(object([
                ("path", Json::from("a.dxf")),
                ("placement", Json::from("alongside")),
                ("local", Json::from(true)),
            ])),
            "say different things",
        );
    }

    // ---- the cases that need the GIS module (always, in the Rust) --------------------------

    #[test]
    fn archive_import_makes_its_coloured_line_and_multiline_leaders_available_to_agents() {
        let fixtures = checkout("tests/archive12d/data");
        let mut client = Client::new();
        client.initialize();
        let palette = client.run_lines(&[&format!(
            "CUSTOMISE \"{}\"",
            crate::geo_fixture::generic(&fixtures.join("drafting_palette.customisation.json"))
        )]);
        assert!(!is_error(&palette), "{}", text_of(&palette));
        let imported = client.call(
            "katana_import",
            object([
                (
                    "path",
                    Json::from(crate::geo_fixture::generic(
                        &fixtures.join("drafting_leaders.12da"),
                    )),
                ),
                ("local", Json::from(true)),
            ]),
        );
        assert!(!is_error(&imported), "{}", text_of(&imported));
        assert_eq!(
            client
                .session
                .document()
                .model()
                .entities
                .find(1)
                .expect("entity 1")
                .color,
            Some(Color::new(224, 128, 32, 255))
        );
        let report = client.call("katana_leaders", object([("scope", Json::from("drawing"))]));
        assert!(!is_error(&report), "{}", text_of(&report));
        let leaders = &report["structuredContent"]["leaders"];
        assert_eq!(leaders.size(), 2);
        assert_eq!(leaders[0]["text"], Json::from("Service note\nFirst line"));
        assert_eq!(
            leaders[0]["lines"],
            strings(&["Service note", "First line"])
        );
        double_eq(double_of(&leaders[0]["landing_angle"]), 0.0);
        assert_eq!(leaders[1]["text"], Json::from("Other note"));
    }

    #[test]
    fn import_local_moves_a_quoted_paths_data_to_the_origin() {
        // The tool quotes the path and adds LOCAL; the session once took
        // LOCAL off and left the quotes on, so local: true failed for every
        // GIS file. In a folder with a blank in its name, so the quotes
        // matter. samples/gis/parcels.geojson spans (180, 0) to (365, 165):
        // moved as one piece to put that lower-left corner at 0,0, it spans
        // (0, 0) to (185, 165).
        let dir = temp("import local");
        let copy = dir.file("site parcels.geojson");
        std::fs::copy(sample("parcels.geojson"), &copy).expect("the sample is copied");
        let mut client = Client::new();
        client.initialize();
        let imported = client.call(
            "katana_import",
            object([("path", Json::from(&copy)), ("local", Json::from(true))]),
        );
        assert!(!is_error(&imported), "{}", text_of(&imported));
        let bounds = client.session.document().model().entities.bounds();
        double_eq(bounds.min.x, 0.0);
        double_eq(bounds.min.y, 0.0);
        double_eq(bounds.max.x, 185.0);
        double_eq(bounds.max.y, 165.0);

        // Without LOCAL the same file keeps its own coordinates.
        let _ = client.call(
            "katana_new_project",
            object([("discard_unsaved_changes", Json::from(true))]),
        );
        let kept = client.call("katana_import", object([("path", Json::from(&copy))]));
        assert!(!is_error(&kept), "{}", text_of(&kept));
        double_eq(
            client.session.document().model().entities.bounds().min.x,
            180.0,
        );

        // placement offset: moved by exactly what is given, (180, 0) to
        // (190, -20.5).
        let _ = client.call(
            "katana_new_project",
            object([("discard_unsaved_changes", Json::from(true))]),
        );
        let moved = client.call(
            "katana_import",
            object([
                ("path", Json::from(&copy)),
                ("placement", Json::from("offset")),
                ("offset_east", Json::from(10)),
                ("offset_north", Json::from(-20.5)),
            ]),
        );
        assert!(!is_error(&moved), "{}", text_of(&moved));
        double_eq(
            client.session.document().model().entities.bounds().min.x,
            190.0,
        );
        double_eq(
            client.session.document().model().entities.bounds().min.y,
            -20.5,
        );
    }

    // katana_export's cloud writes a reference point cloud to a .laz, as GIS >
    // Export Point Cloud does: the scan's 40 000 points (`pdal info`), all
    // held, so no sample and no warning.
    #[test]
    fn export_writes_a_reference_cloud_by_id_or_name() {
        let _shared = shared_data();
        let mut client = Client::new();
        client.initialize();
        let folder = temp("export-cloud");
        let imported = client.call(
            "katana_import",
            object([("path", Json::from(sample("survey_scan.las")))]),
        );
        assert!(!is_error(&imported), "{}", text_of(&imported));
        let exported = client.call(
            "katana_export",
            object([
                ("path", Json::from(folder.file("scan.laz"))),
                ("cloud", Json::from("survey_scan")),
            ]),
        );
        assert!(!is_error(&exported), "{}", text_of(&exported));
        let record = &exported["structuredContent"]["records"][0];
        assert_eq!(record["record"], Json::from("exported"));
        assert_eq!(record["kind"], Json::from("cloud"));
        assert_eq!(record["points"], Json::from(40000));
        // yes/no is a boolean in the records
        assert_eq!(record["sample"], Json::from(false));
        assert!(std::path::Path::new(&folder.file("scan.laz")).exists());
        let by_id = client.call(
            "katana_export",
            object([
                ("path", Json::from(folder.file("again.las"))),
                ("cloud", Json::from(1)),
            ]),
        );
        assert!(!is_error(&by_id), "{}", text_of(&by_id));
        let refused = client.call(
            "katana_export",
            object([
                ("path", Json::from(folder.file("x.las"))),
                ("cloud", Json::from("nothing")),
            ]),
        );
        assert!(is_error(&refused), "{}", text_of(&refused));
    }

    #[test]
    fn import_returns_structured_records() {
        // The reply's records as objects, numbers as numbers: an agent reads
        // the entities that came in, the move LOCAL made and the reference
        // layer's id without reading text. parcels.geojson: 8 features, the
        // spoil heaps a MultiPolygon of two, so 9 entities; its lower-left
        // corner (180, 0) moved to 0,0. terrain.asc: 120 x 90 cells.
        let _shared = shared_data();
        let mut client = Client::new();
        client.initialize();
        let imported = client.call(
            "katana_import",
            object([
                ("path", Json::from(sample("parcels.geojson"))),
                ("placement", Json::from("local")),
            ]),
        );
        assert!(!is_error(&imported), "{}", text_of(&imported));
        let records = &imported["structuredContent"]["records"];
        assert!(records.size() >= 2, "{}", records.dump());
        assert_eq!(records[0]["record"], Json::from("imported"));
        assert_eq!(records[0]["kind"], Json::from("vector"));
        assert_eq!(records[0]["entities"], Json::from(9));
        assert_eq!(records[0]["bounds"], reals(&[0.0, 0.0, 185.0, 165.0]));
        assert_eq!(records[1]["record"], Json::from("placed"));
        assert_eq!(records[1]["placement"], Json::from("local"));
        assert_eq!(records[1]["east"], Json::from(-180));
        assert_eq!(
            imported["structuredContent"]["status"]["entities"],
            Json::from(9)
        );

        let raster = client.call(
            "katana_import",
            object([("path", Json::from(sample("terrain.asc")))]),
        );
        assert!(!is_error(&raster), "{}", text_of(&raster));
        let layer = &raster["structuredContent"]["records"][1];
        assert_eq!(layer["record"], Json::from("reference"));
        assert_eq!(layer["id"], Json::from(1));
        assert_eq!(layer["kind"], Json::from("raster"));
        assert_eq!(layer["width"], Json::from(120));
        assert_eq!(layer["height"], Json::from(90));

        // EXPORT the same way: what was written, as numbers.
        let dir = temp("export records");
        let exported = client.call(
            "katana_export",
            object([("path", Json::from(dir.file("parcels.gpkg")))]),
        );
        assert!(!is_error(&exported), "{}", text_of(&exported));
        let written = &exported["structuredContent"]["records"][0];
        assert_eq!(written["record"], Json::from("exported"));
        assert_eq!(written["driver"], Json::from("GPKG"));
        assert_eq!(written["features"], Json::from(9));
    }

    #[test]
    fn import_takes_its_filter_scope_and_preview_arguments_as_the_words_a_person_types() {
        // tests/geo/data/lots.geojson, by hand: A and B kind=lot, 50 x 40 side
        // by side from (0, 0); C kind=corridor across both at y = 18..22. The
        // box (40, 10) - (60, 30) meets all three; kind = 'lot' leaves A and B.
        let _shared = shared_data();
        let mut client = Client::new();
        client.initialize();
        let lots = lots();
        let previewed = client.call(
            "katana_import",
            object([
                ("path", Json::from(&lots)),
                ("where", Json::from("kind = 'lot'")),
                ("area", integers(&[40, 10, 60, 30])),
                ("clip", Json::from(true)),
                ("preview", Json::from(true)),
            ]),
        );
        assert!(!is_error(&previewed), "{}", text_of(&previewed));
        assert_eq!(
            previewed["structuredContent"]["commands"][0]["command"],
            Json::from(format!(
                "IMPORT \"{lots}\" where=\"kind = 'lot'\" AREA 40,10,60,30 clip PREVIEW"
            ))
        );
        let records = &previewed["structuredContent"]["records"];
        // and any warning GDAL gave
        assert!(records.size() >= 2, "{}", records.dump());
        assert_eq!(records[0]["record"], Json::from("scope"));
        assert_eq!(records[1]["record"], Json::from("import"));
        assert_eq!(records[1]["features"], Json::from(2));
        assert_eq!(records[1]["of"], Json::from(3));
        assert_eq!(
            previewed["structuredContent"]["status"]["entities"],
            Json::from(0)
        );

        let imported = client.call(
            "katana_import",
            object([
                ("path", Json::from(&lots)),
                ("fields", strings(&["name"])),
                ("target_layer", Json::from("site")),
                ("max_features", Json::from(2)),
            ]),
        );
        assert!(!is_error(&imported), "{}", text_of(&imported));
        assert_eq!(
            imported["structuredContent"]["commands"][0]["command"],
            Json::from(format!("IMPORT \"{lots}\" fields=name target=site max=2"))
        );
        assert_eq!(
            imported["structuredContent"]["status"]["entities"],
            Json::from(2)
        );

        let refused = client.call(
            "katana_import",
            object([
                ("path", Json::from(&lots)),
                ("open_options", strings(&["NOPE=1"])),
            ]),
        );
        assert!(is_error(&refused));
        holds(&text_of(&refused), "NOPE");
    }

    #[test]
    fn export_takes_the_shared_scope_and_its_options_as_the_words_a_person_types() {
        // tests/geo/data/lots.geojson imported: three closed polylines on
        // layer lots, the two lots and the corridor (kind=corridor).
        let _shared = shared_data();
        let mut client = Client::new();
        client.initialize();
        let lots = lots();
        assert!(!is_error(
            &client.call("katana_import", object([("path", Json::from(&lots))]))
        ));
        let dir = temp("export options");
        let out = dir.file("lots.gpkg");

        let previewed = client.call(
            "katana_export",
            object([
                ("path", Json::from(&out)),
                ("scope", Json::from("layers")),
                ("layers", strings(&["lots"])),
                ("where", strings(&["PROP=kind:lot"])),
                ("layer_name", Json::from("parcels")),
                ("preview", Json::from(true)),
            ]),
        );
        assert!(!is_error(&previewed), "{}", text_of(&previewed));
        assert_eq!(
            previewed["structuredContent"]["commands"][0]["command"],
            Json::from(format!(
                "EXPORT \"{out}\" LAYERS lots WHERE PROP=kind:lot layername=parcels PREVIEW"
            ))
        );
        let records = &previewed["structuredContent"]["records"];
        assert!(records.size() >= 2, "{}", records.dump());
        assert_eq!(records[0]["record"], Json::from("export"));
        assert_eq!(records[0]["entities"], Json::from(2));
        assert!(!std::path::Path::new(&out).exists());

        let written = client.call(
            "katana_export",
            object([
                ("path", Json::from(&out)),
                ("split_by_layer", Json::from(true)),
            ]),
        );
        assert!(!is_error(&written), "{}", text_of(&written));
        assert_eq!(
            written["structuredContent"]["records"][0]["record"],
            Json::from("exported")
        );
        assert_eq!(
            written["structuredContent"]["records"][0]["features"],
            Json::from(3)
        );
        assert_eq!(
            written["structuredContent"]["records"][0]["layers"],
            Json::from("lots")
        );

        let refused = client.call(
            "katana_export",
            object([
                ("path", Json::from(dir.file("refused.gpkg"))),
                ("layer_creation_options", strings(&["NOPE=1"])),
            ]),
        );
        assert!(is_error(&refused));
        holds(&text_of(&refused), "NOPE");
    }

    // "where" without "scope" filters the selection, as WHERE alone does on
    // the command line - the one scope parser's rule - on katana_export and on
    // a katana_gdal_run input alike; it was refused here and taken there. Two
    // lines drawn, one selected: one line is exported, one buffered.
    #[test]
    fn where_without_a_scope_filters_the_selection_as_the_line_does() {
        let mut client = Client::new();
        client.initialize();
        let _ = client.run_lines(&[
            "LINE 0,0 10,0",
            "LINE 0,5 10,5",
            "RECT 20,0 30,10",
            "SELECT NONE",
            "SELECT 1",
        ]);
        let dir = temp("where alone");
        let out = dir.file("lines.geojson");
        let exported = client.call(
            "katana_export",
            object([
                ("path", Json::from(&out)),
                ("where", strings(&["TYPE=line"])),
            ]),
        );
        assert!(!is_error(&exported), "{}", text_of(&exported));
        assert_eq!(
            exported["structuredContent"]["commands"][0]["command"],
            Json::from(format!("EXPORT \"{out}\" SELECTION WHERE TYPE=line"))
        );
        assert_eq!(
            exported["structuredContent"]["records"][0]["features"],
            Json::from(1),
            "{}",
            exported["structuredContent"]["records"].dump()
        );

        let buffered = client.call(
            "katana_gdal_run",
            object([
                ("algorithm", Json::from("vector buffer")),
                ("arguments", object([("distance", Json::from(1))])),
                (
                    "inputs",
                    object([("input", object([("where", strings(&["TYPE=line"]))]))]),
                ),
                ("output", object([("layer", Json::from("gis/b"))])),
            ]),
        );
        assert!(!is_error(&buffered), "{}", text_of(&buffered));
        assert_eq!(
            buffered["structuredContent"]["line"],
            Json::from(
                "GDAL vector buffer --distance=1 FROM input SELECTION WHERE TYPE=line TO LAYER \
                 gis/b"
            )
        );
        assert_eq!(
            buffered["structuredContent"]["scope"][0]["matched"],
            Json::from(1)
        );

        let layers_alone = client.call(
            "katana_export",
            object([("path", Json::from(&out)), ("layers", strings(&["0"]))]),
        );
        assert!(is_error(&layers_alone), "{}", text_of(&layers_alone));
    }

    #[test]
    fn an_argument_the_tool_does_not_declare_is_refused_by_name_and_nothing_runs() {
        // "split" is not katana_export's word ("split_by_layer" is). Ignored,
        // the call wrote one layer where the agent asked for one per drawing
        // layer.
        let _shared = shared_data();
        let mut client = Client::new();
        client.initialize();
        assert!(!is_error(
            &client.call("katana_import", object([("path", Json::from(lots()))]))
        ));
        let dir = temp("undeclared argument");
        let out = dir.file("lots.gpkg");
        let refused = client.call(
            "katana_export",
            object([("path", Json::from(&out)), ("split", Json::from(true))]),
        );
        assert!(is_error(&refused));
        holds(
            &text_of(&refused),
            "katana_export takes no argument \"split\"",
        );
        holds(&text_of(&refused), "split_by_layer");
        assert!(!std::path::Path::new(&out).exists());
        // A tool that takes no arguments refuses any, saying so.
        let none = client.call("katana_status", object([("verbose", Json::from(true))]));
        assert!(is_error(&none));
        holds(&text_of(&none), "it takes none");
    }

    #[test]
    fn dataset_info_returns_records_and_gdals_json() {
        // terrain.asc's header: 120 x 90 cells of 1.5 from (-5, -5), no-data
        // -9999. The tool changes nothing and reads only the file.
        let _shared = shared_data();
        let mut client = Client::new();
        client.initialize();
        let terrain = sample("terrain.asc");
        let described = client.call(
            "katana_dataset_info",
            object([
                ("path", Json::from(&terrain)),
                ("json", Json::from(true)),
                ("check", Json::from(true)),
            ]),
        );
        assert!(!is_error(&described), "{}", text_of(&described));
        let content = &described["structuredContent"];
        assert_eq!(
            content["line"],
            Json::from(format!("INFO \"{terrain}\" CHECK"))
        );
        // The first record of each kind, as std::map's emplace keeps it.
        let mut first: BTreeMap<String, Json> = BTreeMap::new();
        for record in content["records"].iter() {
            first
                .entry(string_of(&record["record"]))
                .or_insert_with(|| record.clone());
        }
        assert_eq!(first["dataset"]["driver"], Json::from("AAIGrid"));
        assert_eq!(first["raster"]["width"], Json::from(120));
        assert_eq!(
            first["raster"]["bounds"],
            reals(&[-5.0, -5.0, 175.0, 130.0])
        );
        assert_eq!(first["band"]["nodata"], Json::from(-9999));
        assert_eq!(first["check"]["code"], Json::from(0));
        assert_eq!(
            content["gdal"]["raster"]["driverShortName"],
            Json::from("AAIGrid")
        );
        assert_eq!(client.session.document().model().entities.size(), 0);
    }

    #[test]
    fn references_acts_on_a_layer_by_id_or_name_and_lists_them_after() {
        let _shared = shared_data();
        let mut client = Client::new();
        client.initialize();
        assert!(!is_error(&client.call(
            "katana_import",
            object([("path", Json::from(sample("terrain.asc")))])
        )));

        let hidden = client.call(
            "katana_references",
            object([("action", Json::from("hide")), ("id", Json::from(1))]),
        );
        assert!(!is_error(&hidden), "{}", text_of(&hidden));
        assert_eq!(
            hidden["structuredContent"]["line"],
            Json::from("REFS hide 1")
        );
        assert_eq!(
            hidden["structuredContent"]["records"][0]["visible"],
            Json::from(false)
        );
        assert_eq!(hidden["structuredContent"]["references"].size(), 1);
        assert_eq!(
            hidden["structuredContent"]["references"][0]["name"],
            Json::from("terrain")
        );

        let faded = client.call(
            "katana_references",
            object([
                ("action", Json::from("opacity")),
                ("id", Json::from("terrain")),
                ("value", Json::from(0.25)),
            ]),
        );
        assert!(!is_error(&faded), "{}", text_of(&faded));
        assert_eq!(
            faded["structuredContent"]["references"][0]["opacity"],
            Json::from(0.25)
        );

        // Overviews write beside the raster's file: refused unless confirmed.
        let refused = client.call(
            "katana_references",
            object([("action", Json::from("overviews")), ("id", Json::from(1))]),
        );
        assert!(is_error(&refused));
        holds(&text_of(&refused), "confirm");

        let listed = client.call_bare("katana_references");
        assert_eq!(listed["structuredContent"]["line"], Json::from("REFS LIST"));
        assert_eq!(
            listed["structuredContent"]["references"][0]["id"],
            Json::from(1)
        );
    }

    #[test]
    fn undo_and_redo_step_through_the_history() {
        let mut client = Client::new();
        client.initialize();
        let _ = client.run_lines(&["POINT 1,1", "POINT 2,2"]);
        assert_eq!(
            client.call("katana_undo", object([("steps", Json::from(2))]))["structuredContent"]["status"]
                ["entities"],
            Json::from(0)
        );
        assert_eq!(
            client.call("katana_undo", object([("redo", Json::from(true))]))["structuredContent"]["status"]
                ["entities"],
            Json::from(1)
        );
    }

    // What the Sheets editor's Generate Sheets writes reaches an agent as the
    // same line: here its "Rotate the drawing to fill the sheet", rotate=on,
    // on a strip along 45 degrees, which a square plan shows at 1:2000.
    #[test]
    fn the_generate_line_the_sheets_editor_writes_reaches_the_run_commands_tool() {
        let mut client = Client::new();
        client.initialize();
        let result = client.run_lines(&[
            "LINE -152.028,-166.170 166.170,152.028",
            "LINE 166.170,152.028 152.028,166.170",
            "LINE 152.028,166.170 -166.170,-152.028",
            "LINE -166.170,-152.028 -152.028,-166.170",
            "GENERATE fit rotate=on legend=off",
            "VIEW LIST",
        ]);
        assert!(!is_error(&result), "{}", text_of(&result));
        holds(&text_of(&result), "generated 1 sheet: 1");
        holds(&text_of(&result), "scale=1250");
    }

    // The Sheets editor's Sheet Set menu runs SHEETS lines; an agent appends
    // another set's sheets with the same line, renumbered after these.
    #[test]
    fn the_sheet_set_menus_append_reaches_the_run_commands_tool() {
        let mut client = Client::new();
        client.initialize();
        let dir = temp("sheets-append");
        let file = format!("\"{}\"", dir.file("north set.json"));
        let result = client.run_lines(&[
            "SHEET NEW NORTH",
            &format!("SHEETS SAVE {file}"),
            "SHEET RENAME 1 COVER",
            &format!("SHEETS APPEND {file}"),
            "SHEETS",
        ]);
        assert!(!is_error(&result), "{}", text_of(&result));
        holds(
            &text_of(&result),
            &format!("appended 1 sheet from {file}: 2"),
        );
        holds(&text_of(&result), "sheet 2 id=s2 name=\"NORTH\"");
    }

    #[test]
    fn an_older_client_gets_the_facts_as_text_only() {
        let mut client = Client::new();
        let _ = client.request(
            "initialize",
            object([("protocolVersion", Json::from("2024-11-05"))]),
        );
        let result = client.call_bare("katana_status");
        assert!(!result.contains("structuredContent"));
        holds(&text_of(&result), "Entities: 0");
    }

    #[test]
    fn the_help_and_status_are_resources() {
        let mut client = Client::new();
        client.initialize();
        let list = client.request("resources/list", Json::object())["result"]["resources"].clone();
        // The help, the status, the customisation, the surface display styles,
        // the road model and the formats GDAL reads and writes: the Rust app
        // always has the GIS module.
        assert_eq!(list.size(), 6);
        let uris: Vec<String> = list
            .iter()
            .map(|resource| string_of(&resource["uri"]))
            .collect();
        for uri in [
            "katana://help",
            "katana://status",
            "katana://customisation",
            "katana://surface-styles",
            "katana://road-model",
        ] {
            assert!(uris.iter().any(|each| each == uri), "{uri}");
        }
        let help = client.request(
            "resources/read",
            object([("uri", Json::from("katana://help"))]),
        );
        holds(
            &string_of(&help["result"]["contents"][0]["text"]),
            "CUSTOMISE",
        );
        let status = client.request(
            "resources/read",
            object([("uri", Json::from("katana://status"))]),
        );
        assert_eq!(
            Json::parse(string_of(&status["result"]["contents"][0]["text"])).expect("JSON")["entities"],
            Json::from(0)
        );
        assert_eq!(
            client.request(
                "resources/read",
                object([("uri", Json::from("katana://nothing"))])
            )["error"]["code"],
            Json::from(-32002)
        );
    }

    // What katana://surface-styles holds, and that it is JSON's own type.
    #[track_caller]
    fn read_surface_styles(client: &mut Client) -> String {
        let resource = client.request(
            "resources/read",
            object([("uri", Json::from("katana://surface-styles"))]),
        );
        assert_eq!(
            resource["result"]["contents"][0]["mimeType"],
            Json::from("application/json")
        );
        string_of(&resource["result"]["contents"][0]["text"])
    }

    // katana://surface-styles is SURFACESTYLE LIST JSON (docs/mcp.md): the
    // Document's own form, so an agent reads the styles without a line in the
    // history, and a style made through the command tool is there the next
    // read.
    #[test]
    fn the_surface_styles_are_a_resource_as_surface_style_list_json_says_them() {
        let mut client = Client::new();
        client.initialize();
        let styles = Json::parse(read_surface_styles(&mut client)).expect("JSON");
        assert_eq!(styles["format"], Json::from("katana-surface-styles"));
        assert_eq!(styles["default"], Json::from("Standard"));
        // The seven built-in styles (cad's terrain::surface_style).
        assert_eq!(styles["styles"].size(), 7);
        let made = client.run_line("SURFACESTYLE NEW Fine FROM Contours interval=0.5");
        assert!(!is_error(&made), "{}", text_of(&made));
        let listed = client.run_line("SURFACESTYLE LIST JSON");
        let json = output_of(&listed["structuredContent"]["commands"][0]);
        assert_eq!(read_surface_styles(&mut client), json);
        assert_eq!(Json::parse(json).expect("JSON")["styles"].size(), 8);
    }

    // ---- road model (docs/road_design.md) ----
    // katana://road-model is ROADMODEL JSON: what an agent reads the road
    // model by. A corridor of the urban street on a 100 m straight with no
    // surface: its courses and kerbs, each side, the problem that there is no
    // surface for the batters, and the kerb's class as IFC names it.
    #[test]
    fn the_road_model_is_a_resource_as_road_model_json_gives_it() {
        let mut client = Client::new();
        client.initialize();
        let made = client.run_lines(&[
            "ALIGN NEW road 0,0 100,0",
            "ALIGN DESIGN road 0,10 100,10",
            "ROADTEMPLATE DEFAULTS",
            "CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"urban street\"",
            "ROADMODEL JSON",
        ]);
        assert!(!is_error(&made), "{}", text_of(&made));
        let verb = output_of(&made["structuredContent"]["commands"][4]);
        let resource = client.request(
            "resources/read",
            object([("uri", Json::from("katana://road-model"))]),
        );
        let text = string_of(&resource["result"]["contents"][0]["text"]);
        assert_eq!(text, verb);
        let model = Json::parse(text).expect("JSON");
        let mut saw_kerb = false;
        for each in model["objects"].iter() {
            if each["id"] == Json::from("corridor/Main/left/kerb/1") {
                saw_kerb = true;
                assert_eq!(each["ifcClass"], Json::from("IfcKerb"));
                assert_eq!(each["layer"], Json::from("road/kerb"));
            }
        }
        assert!(saw_kerb);
        assert!(!model["problems"].empty());
        holds(
            &string_of(&model["problems"][0]["message"]),
            "no target surface",
        );
    }

    // katana_status and katana://status are the STATUS verb's (cad's
    // document_status), so an agent reads one record whichever front end it
    // drives. The resource's text is written out by hand for this drawing -
    // one rectangle, selected, one undo step, no project, no customisation
    // (the session loads none) - in the form it has always had: every key in
    // alphabetical order, indented by two.
    #[test]
    fn the_status_tool_and_resource_say_what_the_status_verb_says() {
        let mut client = Client::new();
        client.initialize();
        let _ = client.run_lines(&["RECT 0,0 30,20", "SELECT ALL"]);
        let verbs = client.run_lines(&["STATUS", "STATUS JSON"]);
        let lines = &verbs["structuredContent"]["commands"];
        let text = output_of(&lines[0]);
        let json = output_of(&lines[1]);

        let tool = client.call_bare("katana_status");
        assert_eq!(text_of(&tool), text);
        assert_eq!(tool["structuredContent"], Json::parse(&json).expect("JSON"));
        let resource = client.request(
            "resources/read",
            object([("uri", Json::from("katana://status"))]),
        );
        let read = string_of(&resource["result"]["contents"][0]["text"]);
        assert_eq!(read, json);
        assert_eq!(
            read,
            concat!(
                "{\n",
                "  \"alignments\": 0,\n",
                "  \"currentLayer\": \"0\",\n",
                "  \"currentStyle\": \"\",\n",
                "  \"entities\": 1,\n",
                "  \"layers\": 1,\n",
                "  \"modified\": true,\n",
                "  \"project\": null,\n",
                "  \"redoSteps\": 0,\n",
                "  \"selected\": 1,\n",
                "  \"styleLibraryDefinitions\": 0,\n",
                "  \"surveyCodeRules\": 0,\n",
                "  \"undoSteps\": 1\n",
                "}"
            )
        );
        assert_eq!(
            text,
            concat!(
                "Project: (none - not saved to a project yet)  [unsaved changes]\n",
                "Entities: 1  Layers: 1  Alignments: 0\n",
                "Current layer: 0\n",
                "Selected: 1  Undo steps: 1  Redo steps: 0\n",
                "Customisation: 0 linestyles and symbols, 0 survey code rules"
            )
        );
    }

    // ---- the customisation ---------------------------------------------------------------------
    //
    // katana_customisation and katana://customisation (docs/mcp.md, "The
    // customisation"). What is expected is counted from the text of the three
    // committed fixtures and from the format's chapter of
    // docs/customisation.md: a rule and a definition come back as the very
    // objects their file holds.

    // What an agent reads first says how a customisation is loaded and
    // written, and names no file or verb of another program.
    #[test]
    fn the_tool_list_says_how_a_customisation_is_loaded_written_and_read() {
        let mut client = Client::new();
        client.initialize();
        let tools = tools_of(&mut client);
        let commands_description = string_of(&named(&tools, "katana_run_commands")["description"]);
        let customisation = named(&tools, "katana_customisation");
        assert!(!commands_description.is_empty());
        assert!(customisation.is_object());
        let description = string_of(&customisation["description"]);
        for words in [
            "CODE LIST",
            "CODE CHECK",
            "CUSTOMISE <file>",
            "CUSTOMISE EXPORT <file>",
            "HELP CUSTOMISE",
        ] {
            assert!(commands_description.contains(words), "{words}");
        }
        // The round trip that edits one, where the tool that only reads sends
        // it.
        for words in [
            "CUSTOMISE EXPORT <file>",
            "CUSTOMISE <file>",
            "CUSTOMISE REPLACE <file>",
            "katana_run_commands",
        ] {
            assert!(description.contains(words), "{words}");
        }
        // Two things a caller cannot work out and must be told: the summary is
        // one object with none of the lists' envelope, and an entry read here
        // goes into a file only without the one member that is the tool's own.
        for words in [
            "with no part, total, offset or limit",
            "index from a rule, kind from a definition",
        ] {
            assert!(description.contains(words), "{words}");
        }
        // MAPFILE listed and checked the codes until it became CODE LIST and
        // CODE CHECK; it and the two file kinds it was named with no longer
        // exist here.
        for gone in ["MAPFILE", ".mapfile", ".4d"] {
            assert!(!commands_description.contains(gone), "{gone}");
            assert!(!description.contains(gone), "{gone}");
        }
        assert_eq!(customisation["inputSchema"]["required"], strings(&["part"]));
        assert_eq!(
            customisation["inputSchema"]["properties"]["part"]["enum"],
            strings(&["summary", "codes", "definitions", "problems"])
        );
        assert_eq!(
            customisation["annotations"]["readOnlyHint"],
            Json::from(true)
        );
    }

    #[test]
    fn the_customisation_summary_is_cads_report_as_a_tool_and_as_a_resource() {
        let mut client = Client::new();
        client.initialize();
        // A session of no program starts with nothing, and says so.
        let empty = client.call(
            "katana_customisation",
            object([("part", Json::from("summary"))]),
        );
        assert!(!is_error(&empty), "{}", text_of(&empty));
        assert_eq!(empty["structuredContent"]["origin"], Json::from("none"));
        assert_eq!(empty["structuredContent"]["name"], Json::from(""));
        assert_eq!(
            empty["structuredContent"]["counts"]["definitions"],
            Json::from(0)
        );
        assert_eq!(empty["structuredContent"]["counts"]["rules"], Json::from(0));

        load_the_three_customisation_files(&mut client);
        let summary = client.call(
            "katana_customisation",
            object([("part", Json::from("summary"))]),
        );
        assert!(!is_error(&summary), "{}", text_of(&summary));
        let content = &summary["structuredContent"];
        assert_eq!(content["name"], Json::from("test_linestyles"));
        assert_eq!(content["origin"], Json::from("loaded"));
        assert_eq!(content["kept"], Json::from(false));
        // 3 linestyles and 4 symbols
        assert_eq!(content["counts"]["definitions"], Json::from(7));
        assert_eq!(content["counts"]["rules"], Json::from(11));
        assert_eq!(content["counts"]["codes"], Json::from(8));
        // Each file is a source, by the name it declares, with what it brought.
        assert_eq!(content["sources"].size(), 3);
        assert_eq!(content["sources"][0]["name"], Json::from("test_linestyles"));
        assert_eq!(content["sources"][0]["definitions"], Json::from(true));
        assert_eq!(content["sources"][0]["rules"], Json::from(false));
        assert_eq!(content["sources"][1]["name"], Json::from("test_symbols"));
        assert_eq!(content["sources"][2]["name"], Json::from("test_survey"));
        assert_eq!(content["sources"][2]["definitions"], Json::from(false));
        assert_eq!(content["sources"][2]["rules"], Json::from(true));
        // The one name the rules ask for that nothing defines, as the survey
        // fixture's own comment says of its PX* rule.
        assert_eq!(
            content["problems"]["undefined"],
            strings(&["TEST Missing Symbol"])
        );
        // None of the three files says the settings, so they are the defaults.
        assert_eq!(
            content["automation"]["codesOnSurveyImport"],
            Json::from(true)
        );
        assert_eq!(
            content["automation"]["lineworkOnSurveyImport"],
            Json::from(true)
        );
        assert_eq!(content["linework"]["start"], Json::from("ST"));
        assert_eq!(content["linework"]["rectangle"], Json::from("RECT"));
        // It is cad's object and nothing beside it: the lists' envelope is not
        // put round one object, and no member of the tool's own stands among
        // cad's, so the tool, the resource and the verb give one object.
        for own in ["part", "total", "offset", "limit"] {
            assert!(!content.contains(own), "{own}");
        }
        // This session is no program's: it was never started with a host, so
        // nothing went wrong at a start, and nothing was kept to have been made
        // from another built-in.
        assert_eq!(
            content["start"],
            object([
                ("keptFromAnotherBuiltIn", Json::from(false)),
                ("problems", Json::array()),
            ])
        );
        // The text is the same object, for a client without structured content.
        assert_eq!(&Json::parse(text_of(&summary)).expect("JSON"), content);

        // The resource is that object, and both are what CUSTOMISE JSON prints.
        let resource = client.request(
            "resources/read",
            object([("uri", Json::from("katana://customisation"))]),
        );
        assert!(resource.contains("result"), "{}", resource.dump());
        assert_eq!(
            resource["result"]["contents"][0]["mimeType"],
            Json::from("application/json")
        );
        let read = string_of(&resource["result"]["contents"][0]["text"]);
        assert_eq!(&Json::parse(&read).expect("JSON"), content);
        let typed = client.run_line("CUSTOMISE JSON");
        assert!(!is_error(&typed), "{}", text_of(&typed));
        assert_eq!(
            typed["structuredContent"]["commands"][0]["output"],
            Json::from(&read)
        );

        // A summary is one object: an argument that pages or filters a list is
        // refused, not ignored.
        let filtered = client.call(
            "katana_customisation",
            object([
                ("part", Json::from("summary")),
                ("filter", Json::from("tree")),
            ]),
        );
        assert!(is_error(&filtered));
        holds(
            &text_of(&filtered),
            "\"filter\" does not go with part \"summary\"",
        );
    }

    // The indices of the rules a codes filter took, checking that `total`
    // counts them.
    #[track_caller]
    fn code_indices(client: &mut Client, filter: &str) -> Vec<i64> {
        let found = client.call(
            "katana_customisation",
            object([
                ("part", Json::from("codes")),
                ("filter", Json::from(filter)),
            ]),
        );
        let list: Vec<i64> = found["structuredContent"]["codes"]
            .iter()
            .map(|rule| rule["index"].get::<i64>().expect("an index"))
            .collect();
        assert_eq!(
            found["structuredContent"]["total"],
            Json::from(list.len()),
            "{filter}"
        );
        list
    }

    #[test]
    fn the_codes_part_gives_each_rule_as_its_file_holds_it_filtered_and_paged() {
        let mut client = Client::new();
        client.initialize();
        load_the_three_customisation_files(&mut client);
        let all = client.call(
            "katana_customisation",
            object([("part", Json::from("codes"))]),
        );
        assert!(!is_error(&all), "{}", text_of(&all));
        let content = &all["structuredContent"];
        assert_eq!(content["part"], Json::from("codes"));
        assert_eq!(content["total"], Json::from(11));
        assert_eq!(content["offset"], Json::from(0));
        assert_eq!(content["limit"], Json::from(100));
        assert_eq!(content["codes"].size(), 11);
        // Rules 0, 7 and 10 of test_survey, member for member as its text has
        // them - a feature rule, a symbol rule with its object and an
        // attributes rule with its list - each with its place in the map.
        assert_eq!(
            content["codes"][0],
            object([
                ("index", Json::from(0)),
                ("key", Json::from("WM*")),
                ("sets", Json::from("feature")),
                ("layer", Json::from("TEST SERVICES")),
                ("colour", Json::from("blue")),
                ("draw", Json::from("line")),
                ("linestyle", Json::from("TEST Water Main")),
                ("weight", Json::from("0")),
                ("group", Json::from("TEST - SERVICES")),
                ("comment", Json::from("[WM*] Water main")),
            ])
        );
        assert_eq!(
            content["codes"][7],
            object([
                ("index", Json::from(7)),
                ("key", Json::from("AC*")),
                ("sets", Json::from("symbol")),
                ("comment", Json::from("[AC*] Access chamber")),
                ("hide", Json::from(false)),
                (
                    "symbol",
                    object([
                        ("name", Json::from("TEST Survey Mark")),
                        ("colour", Json::from("white")),
                        ("size", Json::from(1.5)),
                    ]),
                ),
            ])
        );
        assert_eq!(
            content["codes"][10],
            object([
                ("index", Json::from(10)),
                ("key", Json::from("*")),
                ("sets", Json::from("attributes")),
                ("comment", Json::from("Every code gets this")),
                (
                    "attributes",
                    Json::array_of([object([
                        ("type", Json::from("text")),
                        ("name", Json::from("Source")),
                        ("value", Json::from("Katana test fixture")),
                    ])]),
                ),
            ])
        );
        assert_eq!(&Json::parse(text_of(&all)).expect("JSON"), content);

        // The filter is a substring of a rule's key, comment or layer, with
        // letter case ignored. "tree" is in the comment of the two TR* rules
        // ("[TR*] Tree") and nowhere else; "test text" is the layer of 1* and
        // 2*; "px*" is the key of the two PX* rules.
        assert_eq!(code_indices(&mut client, "tree"), vec![3, 8]);
        assert_eq!(code_indices(&mut client, "test text"), vec![4, 5]);
        assert_eq!(code_indices(&mut client, "px*"), vec![6, 9]);
        assert_eq!(
            code_indices(&mut client, "no rule says this"),
            Vec::<i64>::new()
        );

        // A page: `total` is still every match, the list only the page.
        let page = client.call(
            "katana_customisation",
            object([
                ("part", Json::from("codes")),
                ("offset", Json::from(9)),
                ("limit", Json::from(5)),
            ]),
        );
        assert_eq!(page["structuredContent"]["total"], Json::from(11));
        assert_eq!(page["structuredContent"]["offset"], Json::from(9));
        assert_eq!(page["structuredContent"]["limit"], Json::from(5));
        assert_eq!(page["structuredContent"]["codes"].size(), 2);
        assert_eq!(
            page["structuredContent"]["codes"][0]["index"],
            Json::from(9)
        );
        assert_eq!(
            page["structuredContent"]["codes"][1]["index"],
            Json::from(10)
        );
        // Past the end is an empty page, not a refusal.
        let past = client.call(
            "katana_customisation",
            object([("part", Json::from("codes")), ("offset", Json::from(11))]),
        );
        assert!(!is_error(&past), "{}", text_of(&past));
        assert_eq!(past["structuredContent"]["total"], Json::from(11));
        assert!(past["structuredContent"]["codes"].empty());

        // What is not a page, a part, or this part's argument is refused by
        // name.
        for bad in [
            object([("part", Json::from("codes")), ("limit", Json::from(0))]),
            object([("part", Json::from("codes")), ("limit", Json::from(1001))]),
            object([("part", Json::from("codes")), ("offset", Json::from(-1))]),
            object([
                ("part", Json::from("codes")),
                ("name", Json::from("TEST Tree")),
            ]),
            object([("part", Json::from("rules"))]),
            Json::object(),
        ] {
            let refused = client.call("katana_customisation", bad.clone());
            assert!(is_error(&refused), "{}", bad.dump());
            assert!(!refused.contains("structuredContent"), "{}", bad.dump());
        }
        holds(
            &text_of(&client.call(
                "katana_customisation",
                object([("part", Json::from("rules"))]),
            )),
            "\"part\" is summary, codes, definitions or problems",
        );
        holds(
            &text_of(&client.call(
                "katana_customisation",
                object([
                    ("part", Json::from("codes")),
                    ("name", Json::from("TEST Tree")),
                ]),
            )),
            "\"name\" picks one definition: it goes with part \"definitions\"",
        );
    }

    // One entry of the definitions list.
    fn definition_entry(
        name: &str,
        kind: &str,
        group: &str,
        units: &str,
        at_vertices: bool,
        from: &str,
    ) -> Json {
        object([
            ("name", Json::from(name)),
            ("kind", Json::from(kind)),
            ("group", Json::from(group)),
            ("units", Json::from(units)),
            ("atVertices", Json::from(at_vertices)),
            ("from", Json::from(from)),
        ])
    }

    #[test]
    fn the_definitions_part_lists_each_definition_and_gives_strokes_only_for_the_one_named() {
        let mut client = Client::new();
        client.initialize();
        load_the_three_customisation_files(&mut client);
        let all = client.call(
            "katana_customisation",
            object([("part", Json::from("definitions"))]),
        );
        assert!(!is_error(&all), "{}", text_of(&all));
        let content = &all["structuredContent"];
        assert_eq!(content["part"], Json::from("definitions"));
        assert_eq!(content["total"], Json::from(7));
        assert_eq!(content["definitions"].size(), 7);
        // In name order, linestyles and symbols together; each says its kind
        // by the list of its file it sits in, its units and whether it is drawn
        // at vertices even where its file leaves them at their default (TEST
        // Water Main says no units, which is world), and the customisation it
        // is from. No entry of a list holds strokes.
        assert_eq!(
            content["definitions"],
            Json::array_of([
                definition_entry(
                    "TEST Dashed Kerb",
                    "linestyle",
                    "Test/Lines",
                    "paper",
                    false,
                    "test_linestyles"
                ),
                definition_entry(
                    "TEST Gate",
                    "linestyle",
                    "Test/Lines",
                    "twoPoint",
                    false,
                    "test_linestyles"
                ),
                definition_entry(
                    "TEST Survey Mark",
                    "symbol",
                    "Test/Marks",
                    "world",
                    true,
                    "test_symbols"
                ),
                definition_entry(
                    "TEST Tree",
                    "symbol",
                    "Test/Vegetation",
                    "world",
                    false,
                    "test_symbols"
                ),
                definition_entry(
                    "TEST U Turn",
                    "symbol",
                    "Test/Marks",
                    "world",
                    true,
                    "test_symbols"
                ),
                definition_entry(
                    "TEST Valve",
                    "symbol",
                    "Test/Marks",
                    "world",
                    true,
                    "test_symbols"
                ),
                definition_entry(
                    "TEST Water Main",
                    "linestyle",
                    "Test/Services",
                    "world",
                    false,
                    "test_linestyles"
                ),
            ])
        );

        // A filter is a substring of a name or a group: three sit in
        // Test/Marks.
        let marks = client.call(
            "katana_customisation",
            object([
                ("part", Json::from("definitions")),
                ("filter", Json::from("MARKS")),
            ]),
        );
        assert_eq!(marks["structuredContent"]["total"], Json::from(3));
        let water = client.call(
            "katana_customisation",
            object([
                ("part", Json::from("definitions")),
                ("filter", Json::from("water")),
            ]),
        );
        assert_eq!(water["structuredContent"]["definitions"].size(), 1);
        assert_eq!(
            water["structuredContent"]["definitions"][0]["name"],
            Json::from("TEST Water Main")
        );

        // Named, it is the one definition whole: the six facts and its
        // strokes, as test_symbols writes them - a move, a draw and an arc of
        // radius -0.5.
        let one = client.call(
            "katana_customisation",
            object([
                ("part", Json::from("definitions")),
                ("name", Json::from("TEST U Turn")),
            ]),
        );
        assert!(!is_error(&one), "{}", text_of(&one));
        assert_eq!(one["structuredContent"]["total"], Json::from(1));
        assert_eq!(one["structuredContent"]["definitions"].size(), 1);
        let mut whole = definition_entry(
            "TEST U Turn",
            "symbol",
            "Test/Marks",
            "world",
            true,
            "test_symbols",
        );
        whole["strokes"] = Json::array_of([
            Json::array_of([Json::from("move"), Json::from(0), Json::from(0)]),
            Json::array_of([Json::from("draw"), Json::from(1), Json::from(0)]),
            Json::array_of([
                Json::from("arc"),
                Json::from(-0.5),
                Json::from(-90),
                Json::from(90),
            ]),
        ]);
        assert_eq!(one["structuredContent"]["definitions"][0], whole);

        // A name is matched as written; one nothing has is refused, naming it,
        // and so is a name together with what narrows a list.
        for missing in ["TEST Nothing", "test u turn"] {
            let refused = client.call(
                "katana_customisation",
                object([
                    ("part", Json::from("definitions")),
                    ("name", Json::from(missing)),
                ]),
            );
            assert!(is_error(&refused), "{missing}");
            holds(
                &text_of(&refused),
                &format!("no definition is called \"{missing}\""),
            );
        }
        let both = client.call(
            "katana_customisation",
            object([
                ("part", Json::from("definitions")),
                ("name", Json::from("TEST U Turn")),
                ("filter", Json::from("marks")),
            ]),
        );
        assert!(is_error(&both));
        holds(&text_of(&both), "\"filter\" does not go with \"name\"");
    }

    #[test]
    fn the_problems_part_is_the_rules_lint_an_object_an_issue() {
        // Three rules, of which one is wrong in one way: KX* is a feature rule
        // with no layer, which the lint warns of (docs/survey_coding.md, the
        // lint table, NoModel) in the words "no layer". Nothing else: red is a
        // standard colour, which needs no front end to say so, "0" is the
        // plain line and cross a symbol Katana draws itself.
        let mut client = Client::new();
        client.initialize();
        let dir = temp("customisation problems");
        let file = dir.file("codes.customisation.json");
        std::fs::write(
            &file,
            r#"{"format": "katana-customisation", "version": 1, "name": "codes", "codes": [
  {"key": "KT*", "sets": "feature", "layer": "KATANA TEST", "colour": "red", "linestyle": "0"},
  {"key": "KX*", "sets": "feature", "colour": "red"},
  {"key": "KT*", "sets": "symbol", "symbol": {"name": "cross", "size": 2}}
]}"#,
        )
        .expect("the customisation is written");
        let loaded = client.run_line(&format!("CUSTOMISE \"{file}\""));
        assert!(!is_error(&loaded), "{}", text_of(&loaded));

        let problems = client.call(
            "katana_customisation",
            object([("part", Json::from("problems"))]),
        );
        assert!(!is_error(&problems), "{}", text_of(&problems));
        let content = &problems["structuredContent"];
        assert_eq!(content["part"], Json::from("problems"));
        assert_eq!(content["total"], Json::from(1));
        assert_eq!(content["rules"], Json::from(3));
        assert_eq!(content["cannotApply"], Json::from(0));
        assert_eq!(content["warnings"], Json::from(1));
        assert_eq!(
            content["problems"],
            Json::array_of([object([
                ("index", Json::from(1)),
                ("key", Json::from("KX*")),
                ("sets", Json::from("feature")),
                ("severity", Json::from("warning")),
                ("kind", Json::from("no layer")),
                (
                    "message",
                    Json::from(
                        "a feature rule with no layer: its codes stay on whatever layer they \
                         are on"
                    ),
                ),
            ])])
        );
        // `index` is the rule's own, the one part codes gives it.
        let codes = client.call(
            "katana_customisation",
            object([("part", Json::from("codes")), ("filter", Json::from("kx"))]),
        );
        assert_eq!(codes["structuredContent"]["codes"].size(), 1);
        assert_eq!(
            codes["structuredContent"]["codes"][0]["index"],
            Json::from(1)
        );

        // A filter narrows the list - by key, kind or message - and not the
        // counts of the whole lint beside it.
        let by_kind = client.call(
            "katana_customisation",
            object([
                ("part", Json::from("problems")),
                ("filter", Json::from("NO LAYER")),
            ]),
        );
        assert_eq!(by_kind["structuredContent"]["total"], Json::from(1));
        let none = client.call(
            "katana_customisation",
            object([
                ("part", Json::from("problems")),
                ("filter", Json::from("unresolved")),
            ]),
        );
        assert_eq!(none["structuredContent"]["total"], Json::from(0));
        assert!(none["structuredContent"]["problems"].empty());
        assert_eq!(none["structuredContent"]["warnings"], Json::from(1));
        assert_eq!(none["structuredContent"]["rules"], Json::from(3));
    }

    // THE way an agent edits a customisation (docs/mcp.md): write the session
    // out as one file, change the JSON, load it in the session's place.
    #[test]
    fn a_customisation_is_edited_by_exporting_it_changing_the_json_and_loading_it_back() {
        let mut client = Client::new();
        client.initialize();
        load_the_three_customisation_files(&mut client);
        let dir = temp("customisation round trip");
        let file = dir.file("session.customisation.json");
        let exported = client.run_line(&format!("CUSTOMISE EXPORT \"{file}\""));
        assert!(!is_error(&exported), "{}", text_of(&exported));

        // The edit: the water main's rule, the first of the eleven, moves to a
        // layer of its own.
        let mut edited =
            Json::parse(std::fs::read(&file).expect("the file is there")).expect("JSON");
        assert_eq!(edited["codes"][0]["key"], Json::from("WM*"));
        assert_eq!(edited["codes"][0]["layer"], Json::from("TEST SERVICES"));
        edited["codes"][0]["layer"] = Json::from("TEST WATER");
        std::fs::write(&file, edited.dump_with(2, ' ', false)).expect("the file is written");

        let replaced = client.run_line(&format!("CUSTOMISE REPLACE \"{file}\""));
        assert!(!is_error(&replaced), "{}", text_of(&replaced));
        let rule = client.call(
            "katana_customisation",
            object([("part", Json::from("codes")), ("filter", Json::from("wm*"))]),
        );
        assert_eq!(rule["structuredContent"]["codes"].size(), 1);
        assert_eq!(
            rule["structuredContent"]["codes"][0]["index"],
            Json::from(0)
        );
        assert_eq!(
            rule["structuredContent"]["codes"][0]["layer"],
            Json::from("TEST WATER")
        );
        // And nothing else went: the same seven definitions, eleven rules and
        // three sources as before the round trip.
        let after = client.call(
            "katana_customisation",
            object([("part", Json::from("summary"))]),
        )["structuredContent"]
            .clone();
        assert_eq!(after["counts"]["definitions"], Json::from(7));
        assert_eq!(after["counts"]["rules"], Json::from(11));
        assert_eq!(after["counts"]["codes"], Json::from(8));
        assert_eq!(after["sources"].size(), 3);
        assert_eq!(after["name"], Json::from("test_linestyles"));
    }

    // A customisation file holding the one entry, in the list it belongs to,
    // its path quoted for a line.
    fn customisation_of(dir: &TempDir, name: &str, list: &str, entry: &Json) -> String {
        let file = dir.file(name);
        let written = object([
            ("format", Json::from("katana-customisation")),
            ("version", Json::from(1)),
            ("name", Json::from("edit")),
            (list, Json::array_of([entry.clone()])),
        ]);
        std::fs::write(&file, written.dump_with(2, ' ', false)).expect("the file is written");
        format!("\"{file}\"")
    }

    fn water_main(client: &mut Client) -> Json {
        client.call(
            "katana_customisation",
            object([("part", Json::from("codes")), ("filter", Json::from("wm*"))]),
        )["structuredContent"]
            .clone()
    }

    fn u_turn(client: &mut Client) -> Json {
        client.call(
            "katana_customisation",
            object([
                ("part", Json::from("definitions")),
                ("name", Json::from("TEST U Turn")),
            ]),
        )["structuredContent"]["definitions"][0]
            .clone()
    }

    // A rule and a definition come back as the objects a file holds - and one
    // member more each, the tool's own: a rule's `index`, a definition's
    // `kind`. A file says neither (a rule's place is its order, a definition's
    // kind the list it is in) and its reader is strict, so an entry copied
    // across whole is refused for that member; with it taken off, it loads.
    #[test]
    fn a_rule_or_a_definition_the_tool_gave_loads_back_once_the_tools_own_member_is_taken_off() {
        let mut client = Client::new();
        client.initialize();
        load_the_three_customisation_files(&mut client);
        let dir = temp("customisation entries back");

        // ---- a rule: the water main's, the first of the eleven.
        let rule = water_main(&mut client)["codes"][0].clone();
        assert_eq!(rule["index"], Json::from(0));
        assert_eq!(rule["layer"], Json::from("TEST SERVICES"));
        // As the tool gives it: refused for `index`, naming the entry, and
        // nothing is loaded.
        let rule_refused = client.run_line(&format!(
            "CUSTOMISE {}",
            customisation_of(&dir, "rule as given.customisation.json", "codes", &rule)
        ));
        assert!(is_error(&rule_refused));
        holds(
            &text_of(&rule_refused),
            "codes[0] \"WM*\": unknown member \"index\"",
        );
        assert_eq!(water_main(&mut client)["codes"][0], rule);
        // Without it - and on another layer, so that the load shows - it
        // loads, and takes the place of the key's rule in that section: still
        // rule 0, still eleven rules.
        let mut edited = rule.clone();
        edited.erase("index").expect("an object");
        edited["layer"] = Json::from("TEST WATER");
        let rule_loaded = client.run_line(&format!(
            "CUSTOMISE {}",
            customisation_of(&dir, "rule.customisation.json", "codes", &edited)
        ));
        assert!(!is_error(&rule_loaded), "{}", text_of(&rule_loaded));
        let mut expected = edited.clone();
        expected["index"] = Json::from(0);
        let after = water_main(&mut client);
        assert_eq!(after["total"], Json::from(1));
        assert_eq!(after["codes"].size(), 1);
        assert_eq!(after["codes"][0], expected);
        assert_eq!(
            client.call(
                "katana_customisation",
                object([("part", Json::from("codes"))])
            )["structuredContent"]["total"],
            Json::from(11)
        );

        // ---- a definition, read whole by its name.
        let definition = u_turn(&mut client);
        assert_eq!(definition["kind"], Json::from("symbol"));
        assert_eq!(definition["strokes"].size(), 3);
        let definition_refused = client.run_line(&format!(
            "CUSTOMISE {}",
            customisation_of(
                &dir,
                "definition as given.customisation.json",
                "symbols",
                &definition
            )
        ));
        assert!(is_error(&definition_refused));
        holds(&text_of(&definition_refused), "unknown member \"kind\"");
        holds(&text_of(&definition_refused), "TEST U Turn");
        assert_eq!(u_turn(&mut client), definition);
        // Without `kind`, in the list that says its kind - and in another
        // group, so that the load shows - it loads in the place of the
        // definition of its name and reads back as the object that was
        // written: a symbol, by that list, and from test_symbols still, since
        // it says so although its file is called "edit". Seven definitions as
        // before.
        let mut plain = definition.clone();
        plain.erase("kind").expect("an object");
        plain["group"] = Json::from("Test/Edited");
        let definition_loaded = client.run_line(&format!(
            "CUSTOMISE {}",
            customisation_of(&dir, "definition.customisation.json", "symbols", &plain)
        ));
        assert!(
            !is_error(&definition_loaded),
            "{}",
            text_of(&definition_loaded)
        );
        let mut moved = definition.clone();
        moved["group"] = Json::from("Test/Edited");
        assert_eq!(u_turn(&mut client), moved);
        assert_eq!(
            client.call(
                "katana_customisation",
                object([("part", Json::from("definitions"))])
            )["structuredContent"]["total"],
            Json::from(7)
        );
    }

    // What the window's Dimension Styles manager sends, an agent sends too:
    // one SET of several fields is one undo step, and INFO reads the style
    // back as a record of every field.
    #[test]
    fn dimension_styles_are_set_in_one_step_and_read_back_as_a_record() {
        let mut client = Client::new();
        client.initialize();
        let made = client.run_lines(&[
            "DIMSTYLE NEW site",
            "DIMSTYLE SET site TEXT 3.5 HEAD Tick PAPER on",
            "DIMSTYLE INFO site",
        ]);
        assert!(!is_error(&made), "{}", text_of(&made));
        let lines = &made["structuredContent"]["commands"];
        assert_eq!(lines.size(), 3);
        holds(
            &output_of(&lines[2]),
            "name=site text=3.5 gap=0.625 extoff=0.625 extbeyond=1.25 arrow=2.5 head=Tick",
        );
        assert_eq!(
            made["structuredContent"]["status"]["undoSteps"],
            Json::from(2),
            "NEW, then the one SET"
        );

        let undone = client.call_bare("katana_undo");
        assert!(!is_error(&undone));
        let info = client.run_lines(&["DIMSTYLE INFO site"]);
        holds(&text_of(&info), "text=2.5");
        holds(&text_of(&info), "head=ClosedFilled");
        holds(&text_of(&info), "paper=off");
    }

    // The line the window's Edit Text sends, sent by an agent: several keys of
    // a text in one TEXTEDIT, its words over two lines, read back by
    // katana_describe_entity with the break written \n.
    #[test]
    fn a_text_is_edited_in_one_step_and_described_with_its_line_breaks() {
        let mut client = Client::new();
        client.initialize();
        let made = client.run_lines(&[
            "TEXTSTYLE NEW Notes paper=3.5",
            "TEXT 10,20 2.5 old",
            r#"TEXTEDIT 1 text="PIT 12\nIL 10.50" style=Notes justify=MC"#,
        ]);
        assert!(!is_error(&made), "{}", text_of(&made));
        assert_eq!(
            made["structuredContent"]["status"]["undoSteps"],
            Json::from(3)
        );
        // Notes is 3.5 mm on paper; at the default 1:1000 that is 3.5 m.
        let described = client.call("katana_describe_entity", object([("id", Json::from(1))]));
        holds(
            &text_of(&described),
            r#"1  Text  layer=0  "PIT 12\nIL 10.50"  height=3.5  style=Notes  justify=MC"#,
        );
    }

    // What the window's rules tab sends, sent by an agent: a rule added with
    // its type, previewed by name with a record per rule of the labels it
    // would have, then switched off - after which it labels nothing, named or
    // not (auto_label, rule_matches).
    #[test]
    fn chosen_label_rules_are_previewed_by_name_with_a_count_for_each() {
        let mut client = Client::new();
        client.initialize();
        let result = client.run_lines(&[
            "LABELSTYLE DEFAULTS",
            "RECT 0,0 20,20",
            "AUTOLABEL RULE ADD lots style=\"Lot Area\" type=Polyline",
            "AUTOLABEL PREVIEW lots",
            "AUTOLABEL RULE SET lots enabled=off",
            "AUTOLABEL PREVIEW lots",
            "LABELSTYLE VALUES area",
        ]);
        assert!(!is_error(&result), "{}", text_of(&result));
        let lines = &result["structuredContent"]["commands"];
        assert_eq!(lines.size(), 7);
        // One label a target (auto_label): the rectangle once for its area.
        assert_eq!(
            output_of(&lines[3]),
            "preview created=1 kept=0 removed=0 skipped=0\nrule=lots labels=1"
        );
        assert_eq!(
            output_of(&lines[5]),
            "preview created=0 kept=0 removed=0 skipped=0"
        );
        holds(&output_of(&lines[6]), "kind=area values=id,layer,code");
        assert_eq!(
            result["structuredContent"]["status"]["entities"],
            Json::from(1),
            "a preview makes nothing"
        );
    }

    // The last line's output of a batch.
    fn last_output(result: &Json) -> String {
        output_of(
            result["structuredContent"]["commands"]
                .as_array()
                .and_then(|lines| lines.last())
                .expect("a line"),
        )
    }

    #[test]
    fn label_layout_names_the_labels_with_no_room_so_an_agent_can_move_them() {
        // Two numbered points at one place (1, 2), labelled in a style that
        // may not move its labels: label 4 finds no room and is named on its
        // own line; pinned elsewhere by LABEL SET, it is placed where it was
        // put - counted as displaced, being away from its own place with a
        // leader back.
        let mut client = Client::new();
        client.initialize();
        let made = client.run_lines(&[
            "LABELSTYLE NEW pt kind=point text={point} displace=off",
            "POINT 0,0",
            "SELECT 1",
            "PROP SET point P1",
            "POINT 0,0",
            "SELECT 2",
            "PROP SET point P2",
            "LABEL 1 2 style=pt",
            "LABEL LAYOUT",
        ]);
        assert!(!is_error(&made), "{}", text_of(&made));
        let layout = last_output(&made);
        holds(&layout, "placed=1 displaced=0 suppressed=1");
        holds(&layout, "\nlabel=4 piece=0 suppressed=yes");

        let moved = client.run_lines(&["LABEL SET 4 at=50,50", "LABEL LAYOUT"]);
        assert!(!is_error(&moved), "{}", text_of(&moved));
        let after = last_output(&moved);
        holds(&after, "placed=2 displaced=1 suppressed=0");
        lacks(&after, "suppressed=yes");
    }

    // What Terrain > Alignment Manager does, as an agent does it: the PIs read
    // back exactly as records, rewritten as one line (one undo step), and the
    // setting-out table a record a station with its key stations named.
    #[test]
    fn an_agent_reads_and_rewrites_an_alignments_p_is_and_its_setting_out_table() {
        let mut client = Client::new();
        client.initialize();
        let result = client.run_lines(&[
            "ALIGN NEW road 0,0 100,0 100,100",
            "ALIGN PIS road",
            "ALIGN PIS road 0,0 100,0,50 100,100",
            "ALIGN STATIONS road 1000",
        ]);
        assert!(!is_error(&result), "{}", text_of(&result));
        let lines = &result["structuredContent"]["commands"];
        assert_eq!(lines.size(), 4);
        holds(
            &output_of(&lines[1]),
            "pi index=1 x=100 y=0 radius=0 spiral_in=0 spiral_out=0",
        );
        holds(&output_of(&lines[2]), "alignment road now has 3 PIs");
        let table = output_of(&lines[3]);
        for key in ["key=start", "key=TC", "key=CT", "key=end"] {
            assert!(table.contains(key), "{key}\n{table}");
        }
        let undone = client.call_bare("katana_undo");
        assert!(!is_error(&undone), "{}", text_of(&undone));
        let back = client.run_lines(&["ALIGN PIS road"]);
        holds(
            &output_of(&back["structuredContent"]["commands"][0]),
            "pi index=1 x=100 y=0 radius=0 ",
        );
    }

    // What Survey > Parcel Report does, as an agent does it: the report, the
    // deed wording under a name, and the labels as one undo step naming their
    // layer.
    #[test]
    fn an_agent_reports_describes_and_labels_a_parcel() {
        let mut client = Client::new();
        client.initialize();
        let result = client.run_lines(&[
            "PLINE 0,0 100,0 100,50 0,50 CLOSE",
            "PARCEL 1",
            "PARCEL 1 LEGAL Lot7",
            "PARCEL 1 LABEL 2",
        ]);
        assert!(!is_error(&result), "{}", text_of(&result));
        let lines = &result["structuredContent"]["commands"];
        assert_eq!(lines.size(), 4);
        holds(
            &output_of(&lines[1]),
            "area 5000.000 m2 (0.500 ha), perimeter 300.000 m",
        );
        holds(&output_of(&lines[2]), "Lot7: Beginning at E 0.000 N 0.000");
        holds(
            &output_of(&lines[3]),
            "5 labels created on layer 0, the current layer",
        );
        assert_eq!(
            result["structuredContent"]["status"]["entities"],
            Json::from(6)
        );
        let undone = client.call_bare("katana_undo");
        assert_eq!(
            undone["structuredContent"]["status"]["entities"],
            Json::from(1)
        );
    }

    // What the Hatch Patterns tab does, as an agent does it: a pattern edited
    // in one step, and a style made hatching with it in another.
    #[test]
    fn an_agent_edits_a_hatch_pattern_and_makes_a_style_that_uses_it() {
        let mut client = Client::new();
        client.initialize();
        let result = client.run_lines(&[
            "HATCH NEW brick 45 0.25",
            "HATCH SET brick 45 0.5 135 0.5",
            "STYLE NEW paving HATCH brick",
        ]);
        assert!(!is_error(&result), "{}", text_of(&result));
        let lines = &result["structuredContent"]["commands"];
        assert_eq!(lines.size(), 3);
        assert_eq!(
            output_of(&lines[1]),
            "hatch pattern brick updated (2 families)"
        );
        assert_eq!(output_of(&lines[2]), "style paving created");
        let refused = client.run_lines(&["HATCH DELETE brick"]);
        assert!(is_error(&refused), "a style still hatches with it");
    }

    // ---- the geoprocessing tools (docs/mcp.md, "Geoprocessing tools") --------------------------

    #[test]
    fn gdal_catalogue_lists_hillshade() {
        let mut client = Client::new();
        client.initialize();
        let result = client.call(
            "katana_gdal_catalogue",
            object([
                ("filter", Json::from("hillshade")),
                ("schemas", Json::from(true)),
            ]),
        );
        assert!(!is_error(&result), "{}", text_of(&result));
        let algorithms = &result["structuredContent"]["algorithms"];
        assert_eq!(algorithms.size(), 1, "{}", algorithms.dump());
        assert_eq!(algorithms[0]["name"], Json::from("raster hillshade"));
        assert_eq!(algorithms[0]["path"], strings(&["raster", "hillshade"]));
        assert_eq!(algorithms[0]["policy"], Json::from("safe"));
        // One algorithm: few enough for its schema to come with it.
        assert!(algorithms[0]["arguments_schema"]["properties"].contains("zfactor"));
        assert!(string_of(&result["structuredContent"]["gdal_version"]).starts_with("3."));
        // A group lists its members and no schemas past 20 of them.
        let raster = client.call(
            "katana_gdal_catalogue",
            object([
                ("filter", Json::from("raster")),
                ("schemas", Json::from(true)),
            ]),
        );
        assert!(raster["structuredContent"]["algorithms"].size() > 20);
        assert!(!raster["structuredContent"]["algorithms"][0].contains("arguments_schema"));
    }

    #[test]
    fn gdal_describe_gives_a_schema_with_bounds() {
        let mut client = Client::new();
        client.initialize();
        let result = client.call(
            "katana_gdal_describe",
            object([("algorithm", Json::from("raster hillshade"))]),
        );
        assert!(!is_error(&result), "{}", text_of(&result));
        let described = &result["structuredContent"];
        let altitude = &described["arguments_schema"]["properties"]["altitude"];
        assert_eq!(altitude["minimum"], Json::from(0.0));
        assert_eq!(altitude["maximum"], Json::from(90.0));
        assert_eq!(
            described["arguments_schema"]["properties"]["zfactor"]["exclusiveMinimum"],
            Json::from(0.0)
        );
        let mut found = false;
        for arg in described["arguments"].iter() {
            if arg["name"] == Json::from("input") {
                found = true;
                assert_eq!(arg["dataset"]["kinds"], strings(&["raster"]));
                assert_eq!(
                    arg["dataset"]["sources"],
                    strings(&["raster", "surface", "file"])
                );
            }
        }
        assert!(found);
        // An alias is taken, as GDAL LIST names it.
        let warp = client.call(
            "katana_gdal_describe",
            object([("algorithm", Json::from("raster warp"))]),
        );
        assert_eq!(
            warp["structuredContent"]["algorithm"]["name"],
            Json::from("raster reproject")
        );
    }

    #[test]
    fn gdal_run_builds_the_line_and_returns_outputs() {
        let mut client = Client::new();
        client.initialize();
        let _ = client.run_lines(&["LINE 0,0 100,0", "TEXT 5,5 2.5 \"LOT 7\""]);
        let result = client.call(
            "katana_gdal_run",
            object([
                ("algorithm", Json::from("vector buffer")),
                (
                    "arguments",
                    object([
                        ("distance", Json::from(1)),
                        ("endcap-style", Json::from("flat")),
                    ]),
                ),
                (
                    "inputs",
                    object([(
                        "input",
                        object([
                            ("scope", Json::from("drawing")),
                            ("where", strings(&["TYPE=line"])),
                        ]),
                    )]),
                ),
                ("output", object([("layer", Json::from("gis/easement"))])),
            ]),
        );
        assert!(!is_error(&result), "{}", text_of(&result));
        let run = &result["structuredContent"];
        assert_eq!(
            run["line"],
            Json::from(
                "GDAL vector buffer --distance=1 --endcap-style=flat FROM input DRAWING WHERE \
                 TYPE=line TO LAYER gis/easement"
            )
        );
        assert!(text_of(&result).starts_with("> GDAL vector buffer"));
        assert_eq!(run["scope"].size(), 1);
        assert_eq!(run["scope"][0]["matched"], Json::from(1));
        assert_eq!(run["scope"][0]["used"], Json::from(1));
        assert_eq!(run["outputs"].size(), 1);
        assert_eq!(run["outputs"][0]["kind"], Json::from("vector"));
        assert_eq!(run["outputs"][0]["layer"], Json::from("gis/easement"));
        assert_eq!(run["outputs"][0]["created"], Json::from(1));
        assert_eq!(run["cancelled"], Json::from(false));
        let listed_reply = client.call_bare("katana_list_entities");
        holds(
            &text_of(&listed_reply),
            "layer=gis/easement  vertices=4  closed  length=204  area=200",
        );
    }

    // An input's schema offers only the sources it reads, and a source of
    // another kind is refused before anything runs: hillshade's input reads
    // rasters, so katana_gdal_describe offers it no drawing scope, and a
    // drawing given to it anyway is refused naming what it takes, with
    // nothing drawn or kept. GDAL answered "Unable to fetch band #1" from the
    // worker.
    #[test]
    fn gdal_run_refuses_a_source_the_input_does_not_read() {
        let mut client = Client::new();
        client.initialize();
        let described = client.call(
            "katana_gdal_describe",
            object([("algorithm", Json::from("raster hillshade"))]),
        )["structuredContent"]
            .clone();
        let offered = &described["inputs_schema"]["properties"]["input"]["properties"];
        assert!(!offered.contains("scope"), "{}", offered.dump());
        assert!(offered.contains("raster"), "{}", offered.dump());
        let _ = client.run_lines(&["RECT 0,0 10,10"]);
        let result = client.call(
            "katana_gdal_run",
            object([
                ("algorithm", Json::from("raster hillshade")),
                (
                    "inputs",
                    object([("input", object([("scope", Json::from("drawing"))]))]),
                ),
                ("output", object([("reference", Json::from("shade"))])),
            ]),
        );
        assert!(is_error(&result), "{}", text_of(&result));
        holds(
            &text_of(&result),
            "input reads raster datasets, and FROM gives it drawing data; it takes \
             raster,surface,file",
        );
        assert_eq!(
            client.call_bare("katana_status")["structuredContent"]["entities"],
            Json::from(1)
        );
    }

    #[test]
    fn gdal_run_quotes_an_output_named_like_a_keyword() {
        // An agent's layer "preview" was written bare: the line read PREVIEW
        // as the flag and was refused. The one quoting rule quotes it.
        let mut client = Client::new();
        client.initialize();
        let _ = client.run_lines(&["LINE 0,0 100,0"]);
        let result = client.call(
            "katana_gdal_run",
            object([
                ("algorithm", Json::from("vector buffer")),
                ("arguments", object([("distance", Json::from(1))])),
                (
                    "inputs",
                    object([("input", object([("scope", Json::from("drawing"))]))]),
                ),
                ("output", object([("layer", Json::from("preview"))])),
            ]),
        );
        assert!(!is_error(&result), "{}", text_of(&result));
        assert_eq!(
            result["structuredContent"]["line"],
            Json::from("GDAL vector buffer --distance=1 FROM input DRAWING TO LAYER \"preview\"")
        );
        assert_eq!(
            result["structuredContent"]["outputs"][0]["layer"],
            Json::from("preview")
        );
    }

    #[test]
    fn gdal_run_refuses_a_confirm_algorithm_without_confirm() {
        let mut client = Client::new();
        client.initialize();
        let folder = temp("gdal-confirm");
        let doomed = folder.file("doomed.txt");
        std::fs::write(&doomed, "x").expect("the file is written");
        let refused = client.call(
            "katana_gdal_run",
            object([
                ("algorithm", Json::from("vsi delete")),
                ("tokens", strings(&[&doomed])),
            ]),
        );
        assert!(is_error(&refused));
        holds(&text_of(&refused), "confirm");
        assert!(std::path::Path::new(&doomed).exists());
        let confirmed = client.call(
            "katana_gdal_run",
            object([
                ("algorithm", Json::from("vsi delete")),
                ("tokens", strings(&[&doomed])),
                ("confirm", Json::from(true)),
            ]),
        );
        assert!(!is_error(&confirmed), "{}", text_of(&confirmed));
        assert!(!std::path::Path::new(&doomed).exists());
    }

    #[test]
    fn gdal_run_refuses_a_pipeline_that_changes_existing_data_unless_told_to() {
        let mut client = Client::new();
        client.initialize();
        let folder = temp("gdal-pipeline");
        let source = folder.file("a.tif");
        let victim = folder.file("b.tif");
        for (file, burn) in [(&source, "7"), (&victim, "1")] {
            let made = client.call(
                "katana_gdal_run",
                object([
                    ("algorithm", Json::from("raster create")),
                    (
                        "tokens",
                        strings(&["--size", "3,3", "--bbox", "0,0,3,3", "--burn", burn, file]),
                    ),
                ]),
            );
            assert!(!is_error(&made), "{}", text_of(&made));
        }
        let bytes = |path: &str| std::fs::read(path).unwrap_or_default();
        let before = bytes(&victim);
        let update_line = format!("read {source} ! update {victim}");
        let overwrite_line = format!("read {source} ! write --overwrite {victim}");

        // An update step, as one quoted word: confirm.
        let update = client.call(
            "katana_gdal_run",
            object([
                ("algorithm", Json::from("raster pipeline")),
                ("tokens", strings(&[&update_line])),
            ]),
        );
        assert!(is_error(&update));
        holds(&text_of(&update), "confirm");
        assert_eq!(bytes(&victim), before);
        // --overwrite inside the pipeline: overwrite.
        let replace = client.call(
            "katana_gdal_run",
            object([
                ("algorithm", Json::from("pipeline")),
                ("tokens", strings(&[&overwrite_line])),
            ]),
        );
        assert!(is_error(&replace));
        holds(&text_of(&replace), "OVERWRITE");
        assert_eq!(bytes(&victim), before);

        let confirmed = client.call(
            "katana_gdal_run",
            object([
                ("algorithm", Json::from("raster pipeline")),
                ("tokens", strings(&[&update_line])),
                ("confirm", Json::from(true)),
            ]),
        );
        assert!(!is_error(&confirmed), "{}", text_of(&confirmed));
        let updated = bytes(&victim);
        assert_ne!(updated, before);
        let replaced = client.call(
            "katana_gdal_run",
            object([
                ("algorithm", Json::from("pipeline")),
                ("tokens", strings(&[&overwrite_line])),
                ("overwrite", Json::from(true)),
            ]),
        );
        assert!(!is_error(&replaced), "{}", text_of(&replaced));
    }

    #[test]
    fn gdal_run_refuses_an_argument_the_algorithm_has_not() {
        let mut client = Client::new();
        client.initialize();
        let refused = client.call(
            "katana_gdal_run",
            object([
                ("algorithm", Json::from("raster hillshade")),
                ("arguments", object([("no-such", Json::from(1))])),
            ]),
        );
        assert!(is_error(&refused));
        holds(&text_of(&refused), "no-such");
    }

    // ---- I2: katana_formats and katana://formats ----

    #[test]
    fn formats_returns_structured_drivers() {
        let mut client = Client::new();
        client.initialize();
        let result = client.call(
            "katana_formats",
            object([
                ("kind", Json::from("vector")),
                ("capability", Json::from("write")),
                ("filter", Json::from("flatgeobuf")),
            ]),
        );
        assert!(!is_error(&result), "{}", text_of(&result));
        let formats = &result["structuredContent"]["formats"];
        assert_eq!(formats.size(), 1, "{}", formats.dump());
        assert_eq!(formats[0]["driver"], Json::from("FlatGeobuf"));
        assert_eq!(formats[0]["write"], strings(&["vector"]));
        assert_eq!(formats[0]["extensions"], strings(&["fgb"]));
        assert!(string_of(&result["structuredContent"]["gdal_version"]).starts_with("3."));
        // The text is the verb's records.
        holds(&text_of(&result), "format driver=FlatGeobuf kind=vector");

        // One driver's options, as IMPORT's oo= and EXPORT's co= are checked.
        let gpkg = client.call("katana_formats", object([("driver", Json::from("GPKG"))]));
        assert!(!is_error(&gpkg), "{}", text_of(&gpkg));
        let mut list_all = false;
        for option in gpkg["structuredContent"]["open_options"].iter() {
            if option["name"] == Json::from("LIST_ALL_TABLES") {
                list_all = true;
                assert_eq!(option["choices"], strings(&["AUTO", "YES", "NO"]));
            }
        }
        assert!(list_all);
        assert!(is_error(&client.call(
            "katana_formats",
            object([("driver", Json::from("NoSuchDriver"))])
        )));
        assert!(is_error(
            &client.call("katana_formats", object([("kind", Json::from("both"))]))
        ));

        // The resource is FORMATS JSON: every format, the same objects.
        let resource = client.request(
            "resources/read",
            object([("uri", Json::from("katana://formats"))]),
        );
        let all = Json::parse(string_of(&resource["result"]["contents"][0]["text"])).expect("JSON");
        assert!(all.is_array());
        let found = all
            .iter()
            .find(|format| format["driver"] == Json::from("FlatGeobuf"))
            .expect("FlatGeobuf is listed");
        assert_eq!(found, &formats[0]);
    }

    // katana_terrain_list: what SURFACE LIST JSON says, as structured content.
    // terrain.asc's least and greatest values, read from the text grid, are
    // 24.892 and 38.819, and a surface of all its 10 800 cells spans exactly
    // them; the grid is read as Float32, within 2e-6 of the text at that size.
    #[test]
    fn terrain_list_gives_the_sessions_surfaces_and_rasters() {
        let _shared = shared_data();
        let mut client = Client::new();
        client.initialize();
        let terrain = sample("terrain.asc");
        let empty = client.call_bare("katana_terrain_list");
        assert!(!is_error(&empty), "{}", text_of(&empty));
        assert!(empty["structuredContent"]["surfaces"].empty());
        let made = client.run_lines(&[
            &format!("IMPORT \"{terrain}\""),
            "SURFACE FROM RASTER 1 NAME ground",
        ]);
        assert!(!is_error(&made), "{}", text_of(&made));
        let listed_reply = client.call_bare("katana_terrain_list");
        assert!(!is_error(&listed_reply), "{}", text_of(&listed_reply));
        let content = &listed_reply["structuredContent"];
        assert_eq!(content["surfaces"].size(), 1, "{}", content.dump());
        assert_eq!(content["surfaces"][0]["name"], Json::from("ground"));
        assert_eq!(content["surfaces"][0]["points"], Json::from(10800));
        near(double_of(&content["surfaces"][0]["zmin"]), 24.892, 2e-6);
        near(double_of(&content["surfaces"][0]["zmax"]), 38.819, 2e-6);
        assert_eq!(content["rasters"].size(), 1);
        assert_eq!(content["rasters"][0]["id"], Json::from(1));
        assert_eq!(content["rasters"][0]["width"], Json::from(120));
        assert_eq!(content["rasters"][0]["cell"], Json::from(1.5));
        holds(&text_of(&listed_reply), "surface ground: ");
        // The style it is drawn in - the drawing's default, Standard, as it
        // has none of its own - and whether the views draw it.
        assert_eq!(content["surfaces"][0]["style"], Json::from("Standard"));
        assert_eq!(content["surfaces"][0]["visible"], Json::from(true));
        let styled = client.run_lines(&["SURFACE STYLE ground Contours", "SURFACE HIDE ground"]);
        assert!(!is_error(&styled), "{}", text_of(&styled));
        let again = client.call_bare("katana_terrain_list")["structuredContent"].clone();
        assert_eq!(again["surfaces"][0]["style"], Json::from("Contours"));
        assert_eq!(again["surfaces"][0]["visible"], Json::from(false));
        // Each of those was one undo step on the drawing, and the surface
        // itself one more: three UNDOs leave none.
        let undone = client.run_lines(&["UNDO 3"]);
        assert!(!is_error(&undone), "{}", text_of(&undone));
        assert!(client.call_bare("katana_terrain_list")["structuredContent"]["surfaces"].empty());
    }

    // T1: CONTOUR is a session line, so the command tool draws contours as the
    // window's Terrain > Analysis > Contours does. terrain.asc's heights run
    // from 24.892 to 38.819, so the whole metres in it are 25 to 38: 14
    // levels.
    #[test]
    fn contours_of_a_surface_are_drawn_through_the_command_tool() {
        let _shared = shared_data();
        let mut client = Client::new();
        client.initialize();
        let terrain = sample("terrain.asc");
        let drawn = client.run_lines(&[
            &format!("SURFACE FROM FILE \"{terrain}\" NAME ground"),
            "CONTOUR SURFACE ground interval=1",
        ]);
        assert!(!is_error(&drawn), "{}", text_of(&drawn));
        let lines = &drawn["structuredContent"]["commands"];
        assert_eq!(lines.size(), 2);
        let reply = output_of(&lines[1]);
        holds(&reply, "contours method=tin cell= levels=14 ");
        holds(&reply, "layer=terrain/contours");
    }

    // A plot's preflight reaches an agent through the command tool: PLOT and
    // PLOTSHEETS are the window's (they paint), but SHEETS CHECK is a session
    // line, and it warns of a surface whose style's contours are too fine to
    // trace - an interval of 0.0005 over the terrain crosses its triangles
    // more than four million times, so a plot would print without them -
    // naming the surface, the style and why (docs/plotting.md, "Preflight
    // checks"). The sheet's plan, 1:1000 about (85, 62), shows the terrain,
    // which covers -4.25 to 174.25 across and -4.25 to 129.25 up.
    #[test]
    fn the_sheet_checks_warn_of_a_surface_whose_contours_are_too_fine_to_plot() {
        let _shared = shared_data();
        let mut client = Client::new();
        client.initialize();
        let terrain = sample("terrain.asc");
        let checked = client.run_lines(&[
            &format!("SURFACE FROM FILE \"{terrain}\" NAME ground"),
            "SURFACESTYLE NEW Fine FROM Contours interval=0.0005",
            "SURFACE STYLE ground Fine",
            "SHEET NEW PLAN",
            "VIEW ADD 1 plan scale=1000 centre=85,62",
            "SHEETS CHECK",
        ]);
        assert!(!is_error(&checked), "{}", text_of(&checked));
        let lines = &checked["structuredContent"]["commands"];
        assert_eq!(lines.size(), 6);
        let reply = output_of(&lines[5]);
        holds(&reply, "warning plan.surface-style 1 vp1 message=\"");
        holds(
            &reply,
            "the surface ground (style Fine) without part of its style: contours: an interval \
             of 0.000500 crosses this surface's triangles more than 4000000 times",
        );
        holds(&reply, " subject=\"ground\"");
    }

    // T2: a shading is a derived reference raster, which katana_terrain_list
    // then lists with the line that made it.
    #[test]
    fn a_shading_made_through_the_command_tool_is_listed_as_a_derived_raster() {
        let _shared = shared_data();
        let mut client = Client::new();
        client.initialize();
        let terrain = sample("terrain.asc");
        let shaded = client.run_lines(&[&format!("RASTER SHADE FILE \"{terrain}\"")]);
        assert!(!is_error(&shaded), "{}", text_of(&shaded));
        let listed_reply = client.call_bare("katana_terrain_list");
        assert!(!is_error(&listed_reply), "{}", text_of(&listed_reply));
        let rasters = &listed_reply["structuredContent"]["rasters"];
        assert_eq!(rasters.size(), 1, "{}", rasters.dump());
        assert_eq!(rasters[0]["name"], Json::from("terrain-hillshade"));
        assert_eq!(rasters[0]["role"], Json::from("derived"));
        holds(&string_of(&rasters[0]["derived_from"]), "RASTER SHADE");
    }

    // T3: slope classes through the command tool, one undo step. terrain.asc's
    // heights span 24.892 to 38.819 on 1.5 m cells, so Horn's gradient, a
    // weighted difference of at most 4 x 13.927 m over 8 x 1.5 m, is at most
    // 4.64 along each axis: under 657 % however it falls. A break at 1000 puts
    // all 120 x 90 cells of 2.25 m2 in [0, 1000): 24 300 m2.
    #[test]
    fn slope_classes_made_through_the_command_tool_are_one_undo_step() {
        let _shared = shared_data();
        let mut client = Client::new();
        client.initialize();
        let terrain = sample("terrain.asc");
        let made = client.run_lines(&[&format!("RASTER SLOPE FILE \"{terrain}\" classes=1000")]);
        assert!(!is_error(&made), "{}", text_of(&made));
        let reply = output_of(&made["structuredContent"]["commands"][0]);
        holds(
            &reply,
            "class name=0-1000 from=0 to=1000 unit=percent area=24300.000",
        );
        let undone = client.run_lines(&["UNDO", "LIST"]);
        assert!(!is_error(&undone), "{}", text_of(&undone));
        holds(&text_of(&undone), "0 entities");
    }

    // T4: statistics by area through the command tool, written on the drawn
    // lot in place. A 40 x 30 m lot on terrain.asc's 1.5 m cells covers
    // 1200 / 2.25 = 533.333 cells by fractional coverage, wherever it lies,
    // and the reply's zone record says so.
    #[test]
    fn statistics_by_area_through_the_command_tool_are_written_on_the_lot() {
        let _shared = shared_data();
        let mut client = Client::new();
        client.initialize();
        let terrain = sample("terrain.asc");
        let made = client.run_lines(&[
            "RECT 10,10 50,40",
            &format!("RASTER ZONAL FILE \"{terrain}\" DRAWING stats=count"),
        ]);
        assert!(!is_error(&made), "{}", text_of(&made));
        let reply = output_of(&made["structuredContent"]["commands"][1]);
        holds(&reply, "zone entity=1 count=533.33");
        holds(&reply, "target=in-place created=0 updated=1");
    }

    // T5: a sight line through the command tool. Two points a metre apart on
    // terrain.asc (1.5 m cells, heights changing by centimetres over a cell):
    // the eye 1.7 m up sees ground a metre off, whatever the slope between.
    #[test]
    fn a_line_of_sight_through_the_command_tool_says_whether_the_target_is_seen() {
        let _shared = shared_data();
        let mut client = Client::new();
        client.initialize();
        let terrain = sample("terrain.asc");
        let looked = client.run_lines(&[&format!(
            "LOS FILE \"{terrain}\" OBSERVER 50,50 TARGET 51,50"
        )]);
        assert!(!is_error(&looked), "{}", text_of(&looked));
        let reply = output_of(&looked["structuredContent"]["commands"][0]);
        holds(
            &reply,
            "sight visible=yes observer=50,50 target=51,50 distance=1.000",
        );
    }

    // The surface edits through the command tool (docs/surface_editing.md): no
    // tool of their own, the lines are the session's. terrain.asc's heights
    // span 24.892 to 38.819 over 120 x 90 cells, every cell centre a vertex
    // (10 800): RAISE 1 moves every one by 1, and katana_terrain_list then
    // reads the drawing's surface at 25.892 to 39.819.
    #[test]
    fn a_surface_edit_through_the_command_tool_is_what_the_terrain_list_reads() {
        let _shared = shared_data();
        let mut client = Client::new();
        client.initialize();
        let terrain = sample("terrain.asc");
        let edited = client.run_lines(&[
            &format!("SURFACE FROM FILE \"{terrain}\" NAME ground"),
            "SURFACE EDIT ground RAISE 1",
        ]);
        assert!(!is_error(&edited), "{}", text_of(&edited));
        let reply = output_of(&edited["structuredContent"]["commands"][1]);
        holds(&reply, "surface name=ground operation=raise");
        holds(
            &reply,
            "moved=10800 held=0 crossings=0 largest_change=1 changed=yes",
        );
        let listed_reply = client.call_bare("katana_terrain_list");
        assert!(!is_error(&listed_reply), "{}", text_of(&listed_reply));
        let surfaces = &listed_reply["structuredContent"]["surfaces"];
        assert_eq!(surfaces.size(), 1, "{}", surfaces.dump());
        near(double_of(&surfaces[0]["zmin"]), 25.892, 1e-6);
        near(double_of(&surfaces[0]["zmax"]), 39.819, 1e-6);
    }

    // DELETE BREAKLINE and MOVE POINT are lines like the other edits, so an
    // agent reaches them by katana_run_commands and reads their records
    // (docs/surface_editing.md): the row y = 51.25 of the sample's lattice
    // from x = 0.25 to 99.25 is 66 edges of a breakline, cleared by the same
    // line; the vertex (49.75, 51.25) in the middle of it, which MOVE POINT
    // refused while the breakline stood, then moves, its level kept (the
    // vertex count is the sample's 10 800 before and after).
    #[test]
    fn an_agent_clears_a_breakline_and_moves_a_vertex_by_the_surface_edit_lines() {
        let _shared = shared_data();
        let mut client = Client::new();
        client.initialize();
        let terrain = sample("terrain.asc");
        let edited = client.run_lines(&[
            &format!("SURFACE FROM FILE \"{terrain}\" NAME ground"),
            "SURFACE EDIT ground BREAKLINE 0.25,51.25 99.25,51.25",
            "SURFACE EDIT ground DELETE BREAKLINE 0.25,51.25 99.25,51.25",
            "SURFACE EDIT ground MOVE POINT 49.75,51.25 TO 49.9,51.4",
        ]);
        assert!(!is_error(&edited), "{}", text_of(&edited));
        let lines = &edited["structuredContent"]["commands"];
        assert_eq!(lines.size(), 4, "{}", lines.dump());
        let cleared = output_of(&lines[2]);
        holds(&cleared, "operation=delete_breakline");
        holds(&cleared, "edges_cleared=66 edges_held=0 changed=yes");
        let moved = output_of(&lines[3]);
        holds(&moved, "operation=move_point");
        holds(&moved, "vertices_before=10800 vertices_after=10800");
        holds(&moved, "moved=1");
        holds(&moved, "changed=yes");
    }

    // IFC through the one door an agent has: the lines File > Export IFC and
    // Import IFC write (docs/ifc.md), each answered in key=value records the
    // agent can read - the preview's objects, the file written, the file
    // read.
    #[test]
    fn an_agent_previews_exports_describes_and_imports_ifc_by_the_dialogs_lines() {
        let mut client = Client::new();
        client.initialize();
        let dir = temp("ifc");
        let file = dir.file("site plan.ifc");
        let exported = client.run_lines(&[
            "LAYER NEW Survey/Kerb",
            "LAYER SET Survey/Kerb",
            "PL 0,0 10,0 20,5",
            &format!("EXPORT \"{file}\" PREVIEW"),
            &format!("EXPORT \"{file}\" NOSURFACES"),
            &format!("INFO \"{file}\""),
            &format!("IFC RULES \"{}\"", dir.file("rules.csv")),
        ]);
        assert!(!is_error(&exported), "{}", text_of(&exported));
        let text = text_of(&exported);
        holds(
            &text,
            "ifc previewed file=\"site plan.ifc\" schema=IFC4X3_ADD2",
        );
        holds(
            &text,
            "object from=\"layer Survey/Kerb\" count=1 class=\"IfcKerb\"",
        );
        holds(&text, "ifc exported file=\"site plan.ifc\"");
        holds(
            &text,
            "ifc described file=\"site plan.ifc\" schema=IFC4X3_ADD2 crs=\"\" entities=1",
        );
        holds(&text, "ifc rules file=\"rules.csv\" rules=");
        assert!(std::path::Path::new(&dir.file("rules.csv")).exists());

        assert!(!is_error(&client.call(
            "katana_new_project",
            object([("discard_unsaved_changes", Json::from(true))])
        )));
        let imported = client.run_lines(&[&format!("IMPORT \"{file}\" NOALIGNMENTS KEEPCRS")]);
        assert!(!is_error(&imported), "{}", text_of(&imported));
        holds(
            &text_of(&imported),
            "ifc imported file=\"site plan.ifc\" schema=IFC4X3_ADD2 crs=\"\" entities=1 \
             alignments=0",
        );
        assert_eq!(
            imported["structuredContent"]["status"]["entities"],
            Json::from(1)
        );
    }

    // ifc (docs/ifc.md, "Road model"): the road model goes out with the
    // drawing unless the line says NOROADMODEL, File > Export IFC's "Road
    // model" box - a word an agent types as the dialog writes it; given twice,
    // refused. The drawing has a corridor, the one thing that gives the model
    // objects: without one the reply has no road record whichever way the
    // word goes, and the test could not tell a word honoured from a word
    // ignored. So the same drawing is previewed both ways - the record (`road
    // elements=N skipped=0 roads=1 parts=P`, N and P above zero) by default,
    // none and no class of the road with the word.
    #[test]
    fn an_agent_leaves_the_road_model_out_of_an_ifc_export_by_its_word() {
        let mut client = Client::new();
        client.initialize();
        let dir = temp("ifc-road");
        let file = dir.file("roadless.ifc");
        let made = client.run_lines(&[
            "LAYER NEW Survey/Kerb",
            "LAYER SET Survey/Kerb",
            "PL 0,0 10,0 20,5",
            "ALIGN NEW road 0,0 100,0",
            "ALIGN DESIGN road 0,10 100,10",
            "ROADTEMPLATE DEFAULTS",
            "CORRIDOR NEW Main ALIGNMENT road TEMPLATE \"urban street\"",
        ]);
        assert!(!is_error(&made), "{}", text_of(&made));

        let written = client.run_lines(&[&format!("EXPORT \"{file}\" PREVIEW")]);
        assert!(!is_error(&written), "{}", text_of(&written));
        let with_road = text_of(&written);
        let record = with_road
            .find("road elements=")
            .unwrap_or_else(|| panic!("{with_road}"));
        assert!(
            with_road[record..].contains(" skipped=0 roads=1 parts="),
            "{with_road}"
        );
        lacks(&with_road, "road elements=0 ");
        holds(&with_road, "class name=IfcRoad count=1");

        let previewed = client.run_lines(&[&format!("EXPORT \"{file}\" NOROADMODEL PREVIEW")]);
        assert!(!is_error(&previewed), "{}", text_of(&previewed));
        holds(
            &text_of(&previewed),
            "ifc previewed file=\"roadless.ifc\" schema=IFC4X3_ADD2",
        );
        // Left out: the reply has no road record and none of the road's
        // classes, while the drawing's own (the polyline, the alignment) is
        // still there.
        lacks(&text_of(&previewed), "road elements=");
        lacks(&text_of(&previewed), "class name=IfcRoad");
        lacks(&text_of(&previewed), "class name=IfcCourse");
        holds(&text_of(&previewed), "class name=IfcAlignment");
        assert!(is_error(&client.run_lines(&[&format!(
            "EXPORT \"{file}\" NOROADMODEL NOROADMODEL"
        )])));
        assert!(!std::path::Path::new(&file).exists());
    }

    // Road templates move between drawings through a file by their own lines
    // (docs/road_design.md, "Template files"): ROADTEMPLATE EXPORT writes the
    // JSON ROADTEMPLATE JSON prints, IMPORT reads it back as one step,
    // refusing a name the drawing has unless OVERWRITE replaces it. An agent
    // has no other door to the file than these lines.
    #[test]
    fn an_agent_moves_road_templates_through_a_file_by_their_lines() {
        let mut client = Client::new();
        client.initialize();
        let dir = temp("road-templates");
        let file = dir.file("templates.json");
        let written = client.run_lines(&[
            "ROADTEMPLATE DEFAULTS",
            "ROADTEMPLATE SET \"urban street\" lane width=3.6",
            &format!("ROADTEMPLATE EXPORT ALL \"{file}\""),
        ]);
        assert!(!is_error(&written), "{}", text_of(&written));
        holds(&text_of(&written), " templates=3");
        assert!(std::path::Path::new(&file).exists());

        // The drawing has all three: refused whole, and said how to replace.
        let refused = client.run_lines(&[&format!("ROADTEMPLATE IMPORT \"{file}\"")]);
        assert!(is_error(&refused));
        holds(&text_of(&refused), "OVERWRITE replaces them");
        // A second export is refused too, until OVERWRITE says so.
        let again = client.run_lines(&[&format!("ROADTEMPLATE EXPORT ALL \"{file}\"")]);
        assert!(is_error(&again));
        let replaced = client.run_lines(&[&format!("ROADTEMPLATE IMPORT \"{file}\" OVERWRITE")]);
        assert!(!is_error(&replaced), "{}", text_of(&replaced));
        holds(&text_of(&replaced), "templates=3 added=0 replaced=3");
    }

    // katana_export of an .ifc hands its records as objects under their whole
    // kind: "ifc exported", with a file field - read as the kind "ifc" and a
    // field "exported file" until the kind became every word before the first
    // key=.
    #[test]
    fn an_ifc_exports_record_is_its_whole_kind_with_its_fields() {
        let mut client = Client::new();
        client.initialize();
        let dir = temp("ifc-records");
        let _ = client.run_lines(&["PL 0,0 10,0 20,5"]);
        let exported = client.call(
            "katana_export",
            object([("path", Json::from(dir.file("site.ifc")))]),
        );
        assert!(!is_error(&exported), "{}", text_of(&exported));
        let records = &exported["structuredContent"]["records"];
        assert!(!records.empty(), "{}", exported.dump());
        assert_eq!(
            records[0]["record"],
            Json::from("ifc exported"),
            "{}",
            records.dump()
        );
        assert_eq!(
            records[0]["file"],
            Json::from("site.ifc"),
            "{}",
            records.dump()
        );
        assert!(!records[0].contains("exported file"), "{}", records.dump());
    }

    // ---- volumes (docs/volumes.md) ----
    // VOLUME is a session line, so the command tool measures between two
    // surfaces as the window's Terrain > Volumes does. The two are closed 3D
    // strings round the square [0, 10]^2 at 10 and at 11, each made a surface
    // of two triangles: a 1 m slab, so 4 x 4 x 1 = 16 m^3 inside the typed
    // square.
    #[test]
    fn a_volume_between_two_surfaces_is_measured_through_the_command_tool() {
        let mut client = Client::new();
        client.initialize();
        let ran = client.run_lines(&[
            "LAYER NEW base",
            "LAYER SET base",
            "PLINE3D 0,0,10 10,0,10 10,10,10 0,10,10 CLOSE",
            "LAYER NEW top",
            "LAYER SET top",
            "PLINE3D 0,0,11 10,0,11 10,10,11 0,10,11 CLOSE",
            "SURFACE FROM LAYERS base ONLY NAME base",
            "SURFACE FROM LAYERS top ONLY NAME top",
            "VOLUME SURFACES base top INSIDE 2,2 6,2 6,6 2,6",
        ]);
        assert!(!is_error(&ran), "{}", text_of(&ran));
        let lines = &ran["structuredContent"]["commands"];
        assert_eq!(lines.size(), 9);
        let reply = output_of(&lines[8]);
        holds(
            &reply,
            "volume method=exact base=base top=top polygons=1 polygon.area=16.000 cut=0.000 \
             fill=16.000 net=16.000 area=16.000",
        );
    }

    // Sections round a curve carry the Pappus correction, and the reply says
    // so: a 1 m slab over [0, 200]^2 and the alignment 20,20 - 180,20 -
    // 180,180 with a curve of radius 40, a strip 10 wide on its inside (the
    // centre is on the left): the tangents hold 120 x 10 = 1200 m^3 each and
    // the curve (pi/4)(40^2 - 30^2) = 175 pi = 549.779, 2949.779 in all,
    // 25 pi = 78.540 less than the 3028.319 along the centreline, which
    // curvature=off gives.
    #[test]
    fn sections_round_a_curve_say_whether_they_were_corrected_for_curvature_and_by_how_much() {
        let mut client = Client::new();
        client.initialize();
        let ran = client.run_lines(&[
            "LAYER NEW base",
            "LAYER SET base",
            "PLINE3D 0,0,10 200,0,10 200,200,10 0,200,10 CLOSE",
            "LAYER NEW top",
            "LAYER SET top",
            "PLINE3D 0,0,11 200,0,11 200,200,11 0,200,11 CLOSE",
            "SURFACE FROM LAYERS base ONLY NAME base",
            "SURFACE FROM LAYERS top ONLY NAME top",
            "ALIGN NEW cl 20,20 180,20 180,180",
            "ALIGN SET cl 1 40",
            "VOLUME SECTIONS base top ALIGNMENT cl interval=5 left=10 right=0",
            "VOLUME SECTIONS base top ALIGNMENT cl interval=5 left=10 right=0 curvature=off",
        ]);
        assert!(!is_error(&ran), "{}", text_of(&ran));
        let lines = &ran["structuredContent"]["commands"];
        assert_eq!(lines.size(), 12);
        let corrected = output_of(&lines[10]);
        holds(&corrected, "curvature=on");
        holds(&corrected, "fill=2949.779");
        holds(&corrected, "curvature.fill=-78.540");
        let plain = output_of(&lines[11]);
        holds(&plain, "curvature=off");
        holds(&plain, "fill=3028.319");
        lacks(&plain, "curvature.fill");
    }

    // ---- road objects (docs/road_objects.md) --------------------------------------------------
    //
    // katana_road_objects: the road objects as structured data, read from the
    // Document. The barrier is the cad tests' own: 20 m along a straight
    // alignment, so its length is 20 by hand.
    #[test]
    fn the_road_objects_tool_gives_each_definition_what_was_made_of_it_and_its_problems() {
        let mut client = Client::new();
        client.initialize();
        let tools = tools_of(&mut client);
        let listed_tool = named(&tools, "katana_road_objects");
        assert!(listed_tool.is_object(), "katana_road_objects is not listed");
        holds(&string_of(&listed_tool["description"]), "ROADOBJECT ADD");
        assert_eq!(listed_tool["annotations"]["readOnlyHint"], Json::from(true));

        let placed = client.run_lines(&[
            "ALIGN NEW Main 0,0 100,0",
            "ALIGN DESIGN Main 0,10 100,10",
            "ROADOBJECT ADD barrier ALIGNMENT Main station=0 to=20 offset=4",
        ]);
        assert!(!is_error(&placed), "{}", text_of(&placed));
        let result = client.call_bare("katana_road_objects");
        assert!(!is_error(&result), "{}", text_of(&result));
        let report = &result["structuredContent"];
        assert_eq!(report["objects"].size(), 1, "{}", report.dump());
        let barrier = &report["objects"][0];
        assert_eq!(barrier["id"], Json::from("barrier-1"));
        assert_eq!(barrier["kind"], Json::from("barrier"));
        assert_eq!(barrier["placement"]["alignment"], Json::from("Main"));
        assert_eq!(barrier["made"].size(), 1);
        assert_eq!(barrier["made"][0]["ifc_class"], Json::from("IfcRailing"));
        assert_eq!(barrier["made"][0]["closed"], Json::from(true));
        near(
            double_of(&barrier["made"][0]["quantities"]["Length"]),
            20.0,
            1e-9,
        );
        assert!(report["problems"].empty());

        // With its alignment gone the barrier is made of nothing, and says
        // why.
        let _ = client.run_lines(&["ALIGN DELETE Main"]);
        let after = client.call_bare("katana_road_objects")["structuredContent"].clone();
        assert!(after["objects"][0]["made"].empty());
        assert_eq!(after["problems"].size(), 1);
        holds(
            &string_of(&after["problems"][0]["message"]),
            "does not exist",
        );
    }

    // The same tool, given the shared scope and filter: the barrier (20 m,
    // offset 4, layer road/objects/barrier) and the sign (station 50, offset
    // 4, layer road/objects/sign) on a straight alignment from (0,0) to
    // (100,0). The sign stands within a metre of (50, 4) and the barrier lies
    // from x = 0 to 20, so the box 45,0,55,10 takes the sign alone.
    #[test]
    fn the_road_objects_tool_takes_the_shared_scope_and_filter_and_says_what_it_took() {
        let mut client = Client::new();
        client.initialize();
        let placed = client.run_lines(&[
            "ALIGN NEW Main 0,0 100,0",
            "ALIGN DESIGN Main 0,10 100,10",
            "ROADOBJECT ADD barrier ALIGNMENT Main station=0 to=20 offset=4",
            "ROADOBJECT ADD sign ALIGNMENT Main station=50 offset=4",
        ]);
        assert!(!is_error(&placed), "{}", text_of(&placed));
        let ids = |result: &Json| -> String {
            result["structuredContent"]["objects"]
                .iter()
                .map(|each| string_of(&each["id"]))
                .collect::<Vec<String>>()
                .join(",")
        };

        // No scope: both, and no "scope" member.
        let everything = client.call_bare("katana_road_objects");
        assert_eq!(ids(&everything), "barrier-1,sign-2");
        assert!(!everything["structuredContent"].contains("scope"));

        // Layers: the sign's, with the record a ROADOBJECT LIST would lead
        // with.
        let by_layer = client.call(
            "katana_road_objects",
            object([
                ("scope", Json::from("layers")),
                ("layers", strings(&["road/objects/sign"])),
            ]),
        );
        assert!(!is_error(&by_layer), "{}", text_of(&by_layer));
        assert_eq!(ids(&by_layer), "sign-2");
        assert_eq!(
            by_layer["structuredContent"]["scope"]["record"],
            Json::from("scope=layers layers=road/objects/sign sublayers=yes matched=1")
        );
        assert_eq!(
            by_layer["structuredContent"]["scope"]["matched"],
            Json::from(1)
        );
        assert_eq!(
            by_layer["structuredContent"]["scope"]["unmade"],
            Json::from(0)
        );

        // An area alone is the scope; its box takes the sign.
        let by_area = client.call(
            "katana_road_objects",
            object([("area", integers(&[45, 0, 55, 10]))]),
        );
        assert!(!is_error(&by_area), "{}", text_of(&by_area));
        assert_eq!(ids(&by_area), "sign-2");
        assert_eq!(
            by_area["structuredContent"]["scope"]["record"],
            Json::from("scope=area area=45,0,55,10 matched=1")
        );

        // "where" with "scope": drawing filters the whole drawing; alone it
        // filters the selection, which holds no road object.
        let barriers = client.call(
            "katana_road_objects",
            object([
                ("scope", Json::from("drawing")),
                ("where", strings(&["PROP=kind:barrier"])),
            ]),
        );
        assert!(!is_error(&barriers), "{}", text_of(&barriers));
        assert_eq!(ids(&barriers), "barrier-1");
        let selected = client.call(
            "katana_road_objects",
            object([("where", strings(&["PROP=kind:barrier"]))]),
        );
        assert_eq!(ids(&selected), "");
        assert_eq!(
            selected["structuredContent"]["scope"]["record"],
            Json::from("scope=selection where=\"PROP=kind:barrier\" matched=0")
        );

        // Taking nothing is a reply, not an error.
        let nothing = client.call(
            "katana_road_objects",
            object([("area", integers(&[200, 200, 300, 300]))]),
        );
        assert!(!is_error(&nothing), "{}", text_of(&nothing));
        assert!(nothing["structuredContent"]["objects"].empty());

        // What is wrong is refused, naming it.
        let missing = client.call(
            "katana_road_objects",
            object([
                ("scope", Json::from("layers")),
                ("layers", strings(&["nosuch"])),
            ]),
        );
        assert!(is_error(&missing));
        holds(&text_of(&missing), "nosuch");
        let entity = client.call(
            "katana_road_objects",
            object([
                ("scope", Json::from("drawing")),
                ("where", strings(&["TYPE=point"])),
            ]),
        );
        assert!(is_error(&entity));
        holds(&text_of(&entity), "TYPE");
        assert!(is_error(&client.call(
            "katana_road_objects",
            object([("layers", strings(&["road"]))])
        )));
    }

    // Selecting by kind is a line, so an agent selects a drawing's leaders and
    // recolours them with the lines a person types (the owner's "select
    // elements by their type such as selecting leaders", 2026-11-03): two
    // leaders and a line, of which SELECT takes the two leaders and COLOR
    // changes only them.
    #[test]
    fn an_agent_selects_the_leaders_by_kind_and_recolours_only_them() {
        let mut client = Client::new();
        client.initialize();
        let result = client.run_lines(&[
            "LEADER 0,0 5,5 text=A",
            "LEADER 0,10 5,15 text=B",
            "LINE 0,0 10,0",
            "SELECT DRAWING WHERE TYPE=leader",
            "COLOR #00FF00",
            "LIST",
        ]);
        assert!(!is_error(&result), "{}", text_of(&result));
        let lines = &result["structuredContent"]["commands"];
        assert_eq!(lines.size(), 6);
        holds(
            &output_of(&lines[3]),
            "scope=drawing where=\"TYPE=leader\" matched=2 selected=2 types=leader:2",
        );
        holds(&output_of(&lines[4]), "2 recoloured");
        assert_eq!(
            client.call_bare("katana_status")["structuredContent"]["selected"],
            Json::from(2)
        );
    }

    // An agent scales the symbols of the styles it selects by one factor (the
    // owner's "change the symbols size", 2026-11-03): the square 0.3 wide that
    // a point's style draws, doubled to 0.6, read back from the style list.
    #[test]
    fn an_agent_scales_the_symbol_sizes_of_the_styles_its_scope_wears() {
        let mut client = Client::new();
        client.initialize();
        let result = client.run_lines(&[
            "STYLE NEW Pit symbol square",
            "STYLE SET Pit symbolsize 0.3",
            "POINT 0,0",
            "SELECT ALL",
            "STYLE APPLY Pit",
            "MODIFY DRAWING SET STYLE.SYMBOLSCALE=2",
            "STYLE LIST",
        ]);
        assert!(!is_error(&result), "{}", text_of(&result));
        let lines = &result["structuredContent"]["commands"];
        assert_eq!(lines.size(), 7);
        holds(
            &output_of(&lines[5]),
            "1 entity matched; changing 1 style. One UNDO restores it.",
        );
        holds(&output_of(&lines[6]), "symbol=square@0.6");
        let refused = client.run_lines(&["MODIFY DRAWING SET STYLE.SYMBOLSCALE=0"]);
        assert!(is_error(&refused));
        let refusal = &refused["structuredContent"]["commands"][0];
        assert_eq!(refusal["ok"], Json::from(false));
        holds(
            &string_of(&refusal["messages"]),
            "symbol scale must be above 0",
        );
    }
}
