//! tests/app/test_view_scale.cpp: its ViewScaleMcp case, a saved view's
//! paper scale changed through katana_run_commands. (ViewScaleSession is the
//! session's: tests/session.)

mod view_scale_mcp {
    use katana_app::mcp::Server;
    use katana_app::{Captured, Session};
    use katana_cad::view_set::{ViewPaperScales, set_view_paper_scales, view_paper_scales};
    use katana_core::json::Json;

    use crate::mcp_client::{double_eq, string_of, strings};

    fn call(server: &mut Server, session: &mut Session, commands: Json) -> Json {
        let message = Json::object_of([
            ("jsonrpc", Json::from("2.0")),
            ("id", Json::from(2)),
            ("method", Json::from("tools/call")),
            (
                "params",
                Json::object_of([
                    ("name", Json::from("katana_run_commands")),
                    ("arguments", Json::object_of([("commands", commands)])),
                ]),
            ),
        ]);
        let response = server.handle(session, message.dump()).expect("a reply");
        Json::parse(response)
            .expect("JSON")
            .find("result")
            .cloned()
            .unwrap_or_else(Json::object)
    }

    #[test]
    fn the_command_tool_changes_only_a_known_saved_view_and_reports_undo() {
        let mut session = Session::new(None, &mut Captured::default());
        set_view_paper_scales(
            session.document_mut(),
            &ViewPaperScales::from([(4, Some(500.0))]),
        )
        .expect("the scales are set");
        let annotation = session.document().annotation_scale();
        let mut server = Server::new("test");
        assert!(
            server
                .handle(
                    &mut session,
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{}}}"#
                )
                .is_some()
        );
        let result = call(
            &mut server,
            &mut session,
            strings(&[
                "VIEWS SCALE LIST",
                "VIEWS SCALE 4 1:250",
                "UNDO",
                "VIEWS SCALE 4",
            ]),
        );
        assert!(result.contains("isError"), "{}", result.dump());
        assert_eq!(result["isError"], Json::from(false), "{}", result.dump());
        let lines = &result["structuredContent"]["commands"];
        assert_eq!(lines.size(), 4);
        assert!(string_of(&lines[0]["output"]).contains("paper_scale=500"));
        assert!(string_of(&lines[1]["output"]).contains("paper_scale=250 changed=yes"));
        assert!(string_of(&lines[3]["output"]).contains("paper_scale=500"));
        double_eq(session.document().annotation_scale(), annotation);
        assert_eq!(session.document().model().entities.size(), 0);

        let unknown = call(
            &mut server,
            &mut session,
            strings(&["VIEWS SCALE 8 500", "POINT 1,1"]),
        );
        assert!(unknown.contains("isError"), "{}", unknown.dump());
        assert_eq!(unknown["isError"], Json::from(true));
        assert_eq!(
            unknown["structuredContent"]["commands"][1]["skipped"],
            Json::from(true)
        );
        assert_eq!(session.document().model().entities.size(), 0);
        let scales = view_paper_scales(session.document()).expect("the scales read");
        assert_eq!(scales, ViewPaperScales::from([(4, Some(500.0))]));
    }
}
