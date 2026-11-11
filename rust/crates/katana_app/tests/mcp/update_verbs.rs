//! tests/app/test_update_verbs.cpp: its three cases of katana_mcp's tool,
//! katana_check_update (mcp::tools' check_update_reply), over the update
//! verbs' rig (tests/support/update_rig.rs). The file's other cases are the
//! session's (tests/session, tests/session_process).

mod update_verbs {
    use katana_app::UpdateSurface;
    use katana_app::mcp::check_update_reply;
    use katana_app::update_verbs::run_update_line;
    use katana_core::ErrorCode;
    use katana_core::json::Json;
    use katana_update::{UpdateState, write_state};

    use crate::update_rig::{
        AVAILABLE, FRESH_STATUS, NOTES, Rig, SIGNATURE_URL, V2_SEQUENCE, http_status, must, tree_of,
    };

    #[test]
    fn katana_mcps_surface_reports_a_release_this_person_set_aside_as_available() {
        // What a person decided about a release is kept per user (katana_update's
        // state) and katana_mcp is given no state folder, so its check is the
        // feed's answer with nothing applied to it: the release the head of the
        // rig works out, available. The person's own program says of the same
        // release `blocked` once it was put back here, and `current` once they
        // skipped it. `blocked` is an answer this surface never gives
        // (docs/mcp.md, "Updates").
        let rig = Rig::new();
        rig.serve_v2();
        let agent = rig.context(UpdateSurface::Mcp);
        assert!(agent.state_directory.as_os_str().is_empty());

        // The release was installed here, did not start and was put back.
        let mut state = UpdateState {
            blocked_sequences: vec![V2_SEQUENCE],
            ..UpdateState::default()
        };
        write_state(&rig.state, &state).expect("the state is written");
        {
            let person = must(run_update_line(&rig.cli(), "UPDATE CHECK"));
            assert!(
                person.text.starts_with("update=blocked "),
                "{}",
                person.text
            );

            let kept = tree_of(&rig.state);
            let checked = must(run_update_line(&agent, "UPDATE CHECK"));
            assert_eq!(checked.text, AVAILABLE);
            let tool = check_update_reply(&agent);
            assert!(!tool.is_error, "{}", tool.text);
            assert_eq!(tool.structured["check"]["update"], Json::from("available"));
            assert!(!tool.structured["check"].contains("reason"));
            assert!(
                !tool.structured["check"].contains("detail"),
                "the sentence that names UPDATE BLOCKED CLEAR is the person's program's alone"
            );
            assert_eq!(
                tree_of(&rig.state),
                kept,
                "and nothing the person keeps was written"
            );
        }

        // The same release, skipped instead.
        state.blocked_sequences.clear();
        state.skipped_version = String::from("0.10.0");
        write_state(&rig.state, &state).expect("the state is written");
        {
            let person = must(run_update_line(&rig.cli(), "UPDATE CHECK"));
            assert_eq!(
                person.text,
                format!(
                    "update=current installed=0.9.0 latest=0.10.0 notes_url={NOTES} \
                     reason=skipped"
                )
            );

            let kept = tree_of(&rig.state);
            let checked = must(run_update_line(&agent, "UPDATE CHECK"));
            assert_eq!(checked.text, AVAILABLE);
            let tool = check_update_reply(&agent);
            assert!(!tool.is_error, "{}", tool.text);
            assert_eq!(tool.structured["check"]["update"], Json::from("available"));
            assert_eq!(tool.structured["status"]["skipped"], Json::from("none"));
            assert_eq!(tree_of(&rig.state), kept);
        }
    }

    // ---- katana_mcp's tool ------------------------------------------------------------------

    #[test]
    fn the_check_update_tool_gives_the_two_records_as_text_and_as_objects() {
        let rig = Rig::new();
        rig.serve_v2();
        let reply = check_update_reply(&rig.context(UpdateSurface::Mcp));
        assert!(!reply.is_error, "{}", reply.text);
        assert_eq!(reply.text, format!("{AVAILABLE}\n{FRESH_STATUS}"));

        let check = &reply.structured["check"];
        assert_eq!(check["update"], Json::from("available"));
        assert_eq!(check["installed"], Json::from("0.9.0"));
        assert_eq!(check["latest"], Json::from("0.10.0"));
        assert_eq!(check["files"], Json::from("5/10"));
        assert_eq!(
            check["download_bytes"],
            Json::from(339),
            "a number, not text"
        );
        assert_eq!(check["notes_url"], Json::from(NOTES));
        assert!(!check.contains("reason"));

        let status = &reply.structured["status"];
        assert_eq!(status["installed"], Json::from("0.9.0"));
        assert_eq!(status["target"], Json::from("win64"));
        assert_eq!(status["kind"], Json::from("archive"));
        assert_eq!(status["staged"], Json::from("none"));
        assert_eq!(status["auto"], Json::from("on"));
        assert_eq!(status["last_check"], Json::from("never"));
        assert_eq!(status["token"], Json::from("absent"));
        assert!(
            !rig.state.exists(),
            "the tool is read-only: no check is recorded"
        );
    }

    #[test]
    fn the_check_update_tool_gives_a_failures_sentence_unquoted() {
        let rig = Rig::new();
        rig.serve_v2();
        rig.feed
            .fail(SIGNATURE_URL, http_status(ErrorCode::NotFound, 404));
        let reply = check_update_reply(&rig.context(UpdateSurface::Mcp));
        assert!(
            !reply.is_error,
            "a feed that cannot be read is an answer: {}",
            reply.text
        );
        assert_eq!(
            reply.structured["check"]["update"],
            Json::from("unreachable")
        );
        assert_eq!(reply.structured["check"]["reason"], Json::from("http_404"));
        assert_eq!(
            reply.structured["check"]["detail"],
            Json::from("the server said 404")
        );
        assert!(!reply.structured["check"].contains("latest"));
    }
}
