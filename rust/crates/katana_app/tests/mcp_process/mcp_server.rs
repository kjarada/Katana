//! tests/app/test_mcp_server.cpp: the six McpServer cases that change what
//! the process shares - the customisation variables a program's session
//! reads, KATANA_UPDATE_TOKEN, the working directory - or read its real
//! standard streams. Each holds start_environment's process lock for its whole
//! length.

mod mcp_server {
    use std::io::Write;
    use std::path::{Path, PathBuf};

    use katana_app::{Captured, Session};
    use katana_cad::customisation_host::built_ins;
    use katana_core::json::Json;

    use crate::app_support::customisation_file;
    use crate::geo_fixture::{TempDir, holds};
    use crate::mcp_client::{Client, is_error, object, string_of, strings, text_of};
    use crate::start_environment::{ScopedVariable, StartEnvironment, process_lock};

    // A directory of its own per test, removed afterwards (the C++'s TempDir).
    fn temp(name: &str) -> TempDir {
        TempDir::new("katana-mcp-", name)
    }

    // Puts the working directory back as it was when the case ends.
    struct WorkingDirectory {
        before: PathBuf,
    }

    impl Drop for WorkingDirectory {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.before);
        }
    }

    #[test]
    fn describe_entity_names_the_entity_whatever_files_the_working_directory_holds() {
        // Files called 1 and #1 where the server runs - a mistyped shell 2>1
        // makes the first - once turned the tool's INFO 1 into INFO <file>,
        // "no importer reads files named ''". It sends INFO #1, always the
        // entity.
        let _process = process_lock();
        let folder = temp("describe-beside-files");
        for name in ["1", "#1"] {
            std::fs::write(folder.file(name), "").expect("the file is written");
        }
        let _restore = WorkingDirectory {
            before: std::env::current_dir().expect("a working directory"),
        };
        std::env::set_current_dir(folder.path()).expect("the working directory moves");
        assert!(Path::new("#1").exists());

        let mut client = Client::new();
        client.initialize();
        let _ = client.run_lines(&["RECT 0,0 10,5"]);
        let described = client.call("katana_describe_entity", object([("id", Json::from(1))]));
        assert!(!is_error(&described), "{}", text_of(&described));
        holds(
            &text_of(&described),
            "1  Polyline  layer=0  vertices=4  closed  length=30  area=50",
        );
    }

    #[test]
    fn the_update_tool_answers_with_the_check_and_status_records_of_this_copy() {
        // This suite's executable is in no install - nothing packaging wrote
        // lies beside it - so the copy is a development one: the check says so
        // without asking any server, and the status says what kind of copy it
        // is (update_verbs). No token of whoever runs the suite is read.
        let _process = process_lock();
        let _token = ScopedVariable::unset("KATANA_UPDATE_TOKEN");
        let mut client = Client::new();
        client.initialize();
        let answered = client.call_bare("katana_check_update");
        assert!(!is_error(&answered), "{}", text_of(&answered));
        let text = text_of(&answered);
        holds(&text, "update=unsupported");
        holds(&text, "reason=development_tree");
        holds(&text, "kind=development");

        let check = &answered["structuredContent"]["check"];
        assert_eq!(check["update"], Json::from("unsupported"));
        assert_eq!(check["reason"], Json::from("development_tree"));
        assert!(check["installed"].is_string());
        assert!(!check.contains("latest"), "nobody was asked");
        let status = &answered["structuredContent"]["status"];
        assert_eq!(status["kind"], Json::from("development"));
        assert_eq!(status["target"], Json::from("none"));
        assert_eq!(status["staged"], Json::from("none"));
        assert_eq!(status["skipped"], Json::from("none"));
        assert_eq!(status["last_check"], Json::from("never"));
        assert_eq!(status["token"], Json::from("absent"));
        assert_eq!(status["last"], Json::from("none"));
        assert_eq!(status["installed"], check["installed"]);

        // It changed nothing in the drawing, and asked twice it answers the
        // same.
        assert!(!client.session.document().is_modified());
        assert_eq!(text_of(&client.call_bare("katana_check_update")), text);

        // An argument it does not take is refused, as for every tool.
        let refused = client.call(
            "katana_check_update",
            object([("download", Json::from(true))]),
        );
        assert!(is_error(&refused));
        holds(
            &text_of(&refused),
            "katana_check_update takes no argument \"download\"",
        );
    }

    // What went wrong when the session STARTED is said once, on standard error
    // - the client's log, which an agent is never shown. Here it is the
    // agent's own case: it copied a rule out of katana_customisation into the
    // file its server keeps the customisation in, `index` and all, and the
    // server was started again. The session starts with the built-in, and the
    // summary told it only `origin: builtIn`.
    #[test]
    fn what_went_wrong_when_the_session_started_is_in_the_summary_and_the_resource() {
        let _process = process_lock();
        let dir = temp("customisation start problem");
        let kept = dir.file("kept.customisation.json");
        std::fs::write(
            &kept,
            r#"{"format": "katana-customisation", "version": 1, "name": "Mine",
 "codes": [{"index": 0, "key": "WM*", "sets": "feature", "layer": "MINE"}]}"#,
        )
        .expect("the kept file is written");
        let symbols = customisation_file("test_symbols.customisation.json");
        let _environment = StartEnvironment::new(Some(&symbols), Some(&kept));
        // What the program's start said: its log, standard error.
        let mut log = Captured::default();
        let started = Session::new(Some("katana_mcp"), &mut log);
        let mut own = Client::over(started);
        own.initialize();

        let summary = own.call(
            "katana_customisation",
            object([("part", Json::from("summary"))]),
        );
        assert!(!is_error(&summary), "{}", text_of(&summary));
        let content = &summary["structuredContent"];
        // The built-in started in the kept file's place: the symbol fixture.
        assert_eq!(content["origin"], Json::from("builtIn"));
        assert_eq!(content["name"], Json::from("test_symbols"));
        assert_eq!(content["counts"]["definitions"], Json::from(4));
        // And why: cad's sentence, which holds the reader's own refusal - the
        // entry and the member - and the file.
        assert_eq!(
            content["start"]["keptFromAnotherBuiltIn"],
            Json::from(false)
        );
        assert_eq!(content["start"]["problems"].size(), 1);
        let problem = string_of(&content["start"]["problems"][0]);
        assert!(
            problem.starts_with(
                "the kept customisation is not read, so the built-in customisation is used: \
                 ParseFailure: codes[0] \"WM*\": unknown member \"index\""
            ),
            "{problem}"
        );
        holds(&problem, "kept.customisation.json");
        // It is the line the log had, and all the log had.
        assert_eq!(log.err, format!("error: {problem}\n"));
        assert_eq!(&Json::parse(text_of(&summary)).expect("JSON"), content);

        // The resource is the same object.
        let resource = own.request(
            "resources/read",
            object([("uri", Json::from("katana://customisation"))]),
        );
        assert!(resource.contains("result"), "{}", resource.dump());
        assert_eq!(
            &Json::parse(string_of(&resource["result"]["contents"][0]["text"])).expect("JSON"),
            content
        );
    }

    #[test]
    fn a_kept_customisation_made_from_another_built_in_is_said_in_the_summary() {
        // The kept file names what it was made from, and that is not this
        // run's built-in (test_symbols). It is the user's, so it is what
        // starts - nothing is wrong - and the summary says the program's own
        // has moved on, which the log's warning said and an agent did not see.
        let _process = process_lock();
        let dir = temp("customisation kept from another");
        let kept = dir.file("kept.customisation.json");
        std::fs::write(
            &kept,
            r#"{"format": "katana-customisation", "version": 1, "name": "Mine",
 "basedOn": {"name": "Some Other", "digest": "0123456789abcdef"},
 "codes": [{"key": "MN*", "sets": "feature", "layer": "MINE"}]}"#,
        )
        .expect("the kept file is written");
        let symbols = customisation_file("test_symbols.customisation.json");
        let _environment = StartEnvironment::new(Some(&symbols), Some(&kept));
        let started = Session::new(Some("katana_mcp"), &mut Captured::default());
        let mut own = Client::over(started);
        own.initialize();

        let summary = own.call(
            "katana_customisation",
            object([("part", Json::from("summary"))]),
        );
        assert!(!is_error(&summary), "{}", text_of(&summary));
        let content = &summary["structuredContent"];
        assert_eq!(content["origin"], Json::from("kept"));
        assert_eq!(content["name"], Json::from("Mine"));
        assert_eq!(content["kept"], Json::from(true));
        assert_eq!(content["builtIn"], Json::from("test_symbols"));
        assert_eq!(
            content["start"],
            object([
                ("keptFromAnotherBuiltIn", Json::from(true)),
                ("problems", Json::array()),
            ])
        );
    }

    // The built-ins the program has (docs/mcp.md, "The customisation"): the
    // summary lists what the build compiled in, the default marked and the one
    // the session is, so that an agent can see what CUSTOMISE BUILTIN <name>
    // would choose from. What this build compiled in is asked of the build.
    #[test]
    fn the_customisation_summary_lists_the_built_ins_the_program_has() {
        let compiled_in = built_ins();
        if compiled_in.len() < 2 {
            eprintln!("this build compiled in fewer than two built-in customisations");
            return;
        }
        let _process = process_lock();
        // A program started with its default, named as the seam takes a name,
        // and no kept file.
        let default_name = compiled_in[0].name.clone();
        let _environment = StartEnvironment::new(Some(&default_name), None);
        let started = Session::new(Some("katana_mcp"), &mut Captured::default());
        let mut own = Client::over(started);
        own.initialize();

        let listed = |content: &Json| -> Vec<String> {
            content["builtIns"]
                .iter()
                .map(|each| string_of(&each["name"]))
                .collect()
        };
        let expected_names: Vec<String> =
            compiled_in.iter().map(|entry| entry.name.clone()).collect();

        let first = own.call(
            "katana_customisation",
            object([("part", Json::from("summary"))]),
        );
        assert!(!is_error(&first), "{}", text_of(&first));
        let content = &first["structuredContent"];
        // Every one the build compiled in, in its order, the default first.
        assert_eq!(listed(content), expected_names);
        assert_eq!(content["builtIns"].size(), compiled_in.len());
        for (at, entry) in compiled_in.iter().enumerate() {
            let each = &content["builtIns"][at];
            assert_eq!(each["source"], Json::from(&entry.source), "{at}");
            assert_eq!(
                each["default"],
                Json::from(at == 0),
                "{}",
                each["name"].dump()
            );
            assert_eq!(
                each["active"],
                Json::from(at == 0),
                "{}",
                each["name"].dump()
            );
            assert!(!each.contains("problem"), "{}", each["name"].dump());
            assert_eq!(each["description"], Json::from(&entry.description), "{at}");
            for count in ["definitions", "rules", "codes"] {
                assert!(
                    each[count].get::<i64>().expect("a count") > 0,
                    "{} {count}",
                    each["name"].dump()
                );
            }
        }
        // The session IS the default, so its own counts are that entry's.
        assert_eq!(content["builtIn"], Json::from(&default_name));
        assert_eq!(content["origin"], Json::from("builtIn"));
        assert_eq!(
            content["builtIns"][0]["definitions"],
            content["counts"]["definitions"]
        );
        assert_eq!(content["builtIns"][0]["rules"], content["counts"]["rules"]);
        assert_eq!(content["builtIns"][0]["codes"], content["counts"]["codes"]);

        // An agent chooses another through the line, and the summary follows:
        // the active one moves, the default does not, and the session is the
        // other.
        let other = compiled_in[1].name.clone();
        let chosen = own.call(
            "katana_run_commands",
            object([(
                "commands",
                strings(&[&format!("CUSTOMISE BUILTIN \"{other}\"")]),
            )]),
        );
        assert!(!is_error(&chosen), "{}", text_of(&chosen));
        let second = own.call(
            "katana_customisation",
            object([("part", Json::from("summary"))]),
        );
        let after = &second["structuredContent"];
        assert_eq!(listed(after), expected_names);
        assert_eq!(after["name"], Json::from(&other));
        assert_eq!(after["origin"], Json::from("builtIn"));
        assert_eq!(after["builtIns"][0]["default"], Json::from(true));
        assert_eq!(after["builtIns"][0]["active"], Json::from(false));
        assert_eq!(after["builtIns"][1]["default"], Json::from(false));
        assert_eq!(after["builtIns"][1]["active"], Json::from(true));

        // The resource, the text and the tool are one report.
        let resource = own.request(
            "resources/read",
            object([("uri", Json::from("katana://customisation"))]),
        );
        assert!(resource.contains("result"), "{}", resource.dump());
        assert_eq!(
            &Json::parse(string_of(&resource["result"]["contents"][0]["text"])).expect("JSON"),
            after
        );
        assert_eq!(&Json::parse(text_of(&second)).expect("JSON"), after);
    }

    // ---- the real streams --------------------------------------------------------------------
    //
    // The C++ captured the process's stdout and stderr around the call. A
    // capture of the real streams cannot be taken beside libtest's other
    // threads (its harness writes to them as cases end), so the case runs
    // itself as a child of this test program, alone, and reads the child's
    // streams: between two marks the child writes, nothing may appear.

    // Set in the child's environment by the parent: run the calls, not the
    // check.
    const CHILD: &str = "KATANA_MCP_REAL_STREAMS_CHILD";
    const BEGIN: &str = "<<katana_mcp real streams begin>>";
    const END: &str = "<<katana_mcp real streams end>>";

    fn mark(text: &str) {
        let mut out = std::io::stdout().lock();
        let _ = out.write_all(text.as_bytes());
        let _ = out.flush();
        drop(out);
        let mut err = std::io::stderr().lock();
        let _ = err.write_all(text.as_bytes());
        let _ = err.flush();
    }

    // What a stream held between the two marks; None when they are not both
    // there.
    fn between_marks(text: &str) -> Option<String> {
        let start = text.find(BEGIN)? + BEGIN.len();
        let end = text[start..].find(END)? + start;
        Some(String::from(&text[start..end]))
    }

    #[test]
    fn nothing_a_command_prints_leaks_onto_the_real_streams() {
        let _process = process_lock();
        if std::env::var_os(CHILD).is_some() {
            let mut client = Client::new();
            client.initialize();
            mark(BEGIN);
            let _ = client.run_lines(&["POINT 1,1", "CIRCLE 0,0 -1"]);
            mark(END);
            return;
        }
        // This case's name as libtest knows it: its module path less the
        // crate's (the target's) name.
        let path = module_path!().split_once("::").map_or("", |(_, rest)| rest);
        let name = format!("{path}::nothing_a_command_prints_leaks_onto_the_real_streams");
        let child = std::process::Command::new(std::env::current_exe().expect("this program"))
            .args([name.as_str(), "--exact", "--nocapture", "--test-threads=1"])
            .env(CHILD, "1")
            .output()
            .expect("the child runs");
        let stdout = String::from_utf8_lossy(&child.stdout);
        let stderr = String::from_utf8_lossy(&child.stderr);
        assert!(
            child.status.success(),
            "--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}"
        );
        assert_eq!(
            between_marks(&stdout).as_deref(),
            Some(""),
            "stdout: {stdout}"
        );
        assert_eq!(
            between_marks(&stderr).as_deref(),
            Some(""),
            "stderr: {stderr}"
        );
    }
}
