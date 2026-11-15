//! tests/app/test_session.cpp: the session as katana_cli runs a line - what
//! goes to `out`, where a reply or a report goes, and what to `err`, where a
//! refusal goes. The verbs' own text is pinned by the cli tests; this pins the
//! streams, which a merged ctest log cannot. The start-up cases are in
//! tests/session_process (they set the customisation variables).

mod session {
    use katana_app::{Captured, Session};
    use katana_core::ErrorCode;

    use crate::app_support::{ScratchDirectory, contains, generic, run};

    // Whether Session::run reads a line that is not UTF-8 as Windows-1252: it
    // does on Windows, where the bytes a program is handed are the ANSI code
    // page's; elsewhere the Rust session refuses such a line at its door, in
    // a sentence that says it is not valid UTF-8 (the C++ hands the bytes on,
    // and a name is refused in the same words by its table).
    const READS_ANSI_LINES: bool = cfg!(windows);

    fn session_of_no_program() -> Session {
        Session::new(None, &mut Captured::default())
    }

    // ---- the streams --------------------------------------------------------------------------

    #[test]
    fn a_refusal_that_carries_a_report_prints_the_report_where_a_report_goes() {
        // UTILITY CHECK refuses a schedule with errors against its schema, so
        // that a script stops there, and the refusal carries the whole check. A
        // script that keeps stdout - katana_cli -c "UTILITY CHECK ..." >
        // check.txt - must have the check exactly when it failed, as before the
        // verb moved into the interpreter; the refusal itself is one line on
        // stderr.
        let scratch = ScratchDirectory::new("check");
        let schema = scratch.file(
            "schema.csv",
            "kind,attribute,value,detail,label\n\
             schema,Example Utility Schema,0.1,,\n\
             identifier,AssetId,,,\n\
             field,AssetId,Alphanumerical,Yes,Asset Id\n\
             field,Status,Domain List: Status,Yes,Status\n\
             domain,Status,Live,,\n\
             domain,Status,Dead,,\n",
        );
        let wrong = scratch.file("wrong.csv", "AssetId,Status,easting\nW-1,Alive,1\nW-2,,1\n");
        let mut session = session_of_no_program();

        let failed = run(
            &mut session,
            format!("UTILITY CHECK {wrong} SCHEMA {schema}"),
        );
        assert!(!failed.ok);
        assert_eq!(
            failed.err,
            "error: InvalidArgument: the schedule does not meet the schema: 2 errors\n"
        );
        assert!(
            failed
                .out
                .starts_with("Delivery schema check against Example Utility Schema v0.1\n"),
            "{}",
            failed.out
        );
        assert!(contains(
            &failed.out,
            "2 rows, 2 assets (AssetId): 2 errors, 0 warnings"
        ));
        assert!(
            contains(&failed.out, "\"Alive\" is not in the domain"),
            "{}",
            failed.out
        );
        assert!(contains(&failed.out, "Status is empty"));
        assert!(failed.out.ends_with('\n'));

        // A schedule that passes is a reply, on stdout, as it always was.
        let clean = scratch.file("clean.csv", "AssetId,Status,easting\nW-1,Live,1\n");
        let passed = run(
            &mut session,
            format!("UTILITY CHECK {clean} SCHEMA {schema}"),
        );
        assert!(passed.ok);
        assert_eq!(passed.err, "");
        assert!(contains(
            &passed.out,
            "1 rows, 1 assets (AssetId): 0 errors, 0 warnings"
        ));

        // A refusal of one line is that line on stderr and nothing on stdout.
        let refused = run(&mut session, "CIRCLE 0,0 -1");
        assert!(!refused.ok);
        assert_eq!(refused.out, "");
        assert!(refused.err.starts_with("error: "), "{}", refused.err);
        assert_eq!(
            refused.err.find('\n'),
            Some(refused.err.len() - 1),
            "{}",
            refused.err
        );
    }

    #[test]
    fn a_line_that_is_not_utf8_is_read_as_windows1252_on_windows_and_said_once_and_becomes_a_name()
    {
        // What a pipe from a program that writes the ANSI code page, or a script
        // saved from an ANSI editor, hands the session: 0xE9 for e-acute, 0xB0
        // for the degree sign (Windows-1252, CP1252.TXT) - U+00E9 and U+00B0,
        // UTF-8 C3 A9 and C2 B0. Refused, as a layer's name once was, it turns a
        // keyboard away; passed on, it ended the process at the next JSON
        // writer.
        let mut session = session_of_no_program();
        let first = run(&mut session, b"LAYER NEW Caf\xE9");
        if !READS_ANSI_LINES {
            // Elsewhere the session's door is what answers, with a sentence.
            assert!(!first.ok);
            assert!(contains(&first.err, "not valid UTF-8"), "{}", first.err);
            return;
        }
        assert!(first.ok, "{}", first.err);
        assert!(
            contains(&first.err, "Windows-1252"),
            "the guess is said: {}",
            first.err
        );
        assert!(
            contains(&first.out, "layer Caf\u{e9} created"),
            "{}",
            first.out
        );

        let second = run(&mut session, b"LAYER NEW Pente\xB0");
        assert!(second.ok, "{}", second.err);
        assert!(
            !contains(&second.err, "Windows-1252"),
            "once a session, not once a line"
        );
        assert!(
            contains(&second.out, "layer Pente\u{b0} created"),
            "{}",
            second.out
        );

        // A line that already is UTF-8 is not read a second time as two letters.
        let utf8 = run(&mut session, "LAYER NEW Gr\u{fc}\u{df}");
        assert!(utf8.ok);
        assert!(
            contains(&utf8.out, "layer Gr\u{fc}\u{df} created"),
            "{}",
            utf8.out
        );
    }

    #[test]
    fn a_style_named_in_the_ansi_code_page_lists_as_json_and_saves_without_ending_the_process() {
        // SURFACESTYLE NEW Pente + 0xB0, then SURFACESTYLE LIST JSON and SAVE,
        // threw nlohmann's type_error.316 out of Session::run (exit 127 in
        // katana_cli, the session's work lost). On Windows the name is now
        // text, UTF-8 from the first line on; elsewhere the session's door
        // refuses it. Either way nothing is left in the model for a JSON writer
        // to fail on, and nothing panics.
        let scratch = ScratchDirectory::new("ansi-style");
        let mut session = session_of_no_program();
        let made = run(&mut session, b"SURFACESTYLE NEW Pente\xB0");
        assert_eq!(made.ok, READS_ANSI_LINES, "{}", made.err);
        if !READS_ANSI_LINES {
            assert!(contains(&made.err, "not valid UTF-8"), "{}", made.err);
        }
        let listed = run(&mut session, "SURFACESTYLE LIST JSON");
        assert!(listed.ok, "{}", listed.err);
        if READS_ANSI_LINES {
            assert!(
                contains(&listed.out, "\"name\":\"Pente\u{b0}\""),
                "{}",
                listed.out
            );
        }
        assert!(run(&mut session, "LEADER LIST JSON").ok);
        let saved = run(
            &mut session,
            format!("SAVE \"{}\"", generic(&scratch.path.join("p"))),
        );
        assert!(saved.ok, "{}", saved.err);
        assert!(contains(&saved.out, "saved to"), "{}", saved.out);
    }

    #[test]
    fn a_non_utf8_path_in_a_line_is_an_answer_not_an_exception_out_of_run() {
        // OPEN of a path with 0xE9 in it handed std::filesystem bytes it throws
        // on, and the session let it out: katana_cli ended and an MCP --project
        // ended before it answered. The line is an answer like any other: read
        // as text first on Windows, where it names a folder that is not there,
        // and refused at the session's door elsewhere.
        let scratch = ScratchDirectory::new("ansi-path");
        let mut session = session_of_no_program();
        let mut line = format!("OPEN \"{}/Jos", generic(&scratch.path)).into_bytes();
        line.extend_from_slice(b"\xE9\"");
        let opened = run(&mut session, line);
        assert!(!opened.ok);
        assert!(contains(&opened.err, "error: "), "{}", opened.err);
        // And the script reader: a script saved in the ANSI page is text, its
        // path a UTF-8 path, and a missing one is a Status that names it.
        let mut said = Captured::default();
        let missing =
            Session::read_script(format!("{}/none.kcs", generic(&scratch.path)), &mut said);
        let missing = missing.expect_err("a missing script is refused");
        assert_eq!(missing.code, ErrorCode::NotFound);
        let _ = scratch.file("ansi.kcs", b"LAYER NEW Caf\xE9\r\n\r\nLAYER LIST\r\n");
        let mut said = Captured::default();
        let lines = Session::read_script(format!("{}/ansi.kcs", generic(&scratch.path)), &mut said)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(
            lines.len(),
            3,
            "CRLF ends a line and a blank line in the middle is kept"
        );
        assert_eq!(lines[0], "LAYER NEW Caf\u{e9}");
        assert_eq!(lines[1], "");
        assert_eq!(lines[2], "LAYER LIST");
        assert!(contains(&said.err, "Windows-1252"), "{}", said.err);
    }

    #[test]
    fn info_with_an_entity_id_describes_the_entity_in_every_build() {
        // With the GIS module every INFO was once read as INFO <file>, before
        // the interpreter could see it, so INFO 1 - and katana_describe_entity,
        // which sends it - answered that the file did not exist. A 10 x 5
        // rectangle: perimeter 30, area 50.
        let mut session = session_of_no_program();
        assert!(run(&mut session, "RECT 0,0 10,5").ok);
        let info = run(&mut session, "INFO 1");
        assert!(info.ok, "{}", info.err);
        assert_eq!(
            info.out,
            "1  Polyline  layer=0  vertices=4  closed  length=30  area=50\n"
        );
        assert_eq!(info.err, "");
        assert_eq!(run(&mut session, "info #1").out, info.out);

        let missing = run(&mut session, "INFO 2");
        assert!(!missing.ok);
        assert_eq!(missing.err, "error: NotFound: entity does not exist [2]\n");
    }
}
