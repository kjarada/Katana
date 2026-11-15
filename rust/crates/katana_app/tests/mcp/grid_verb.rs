//! tests/geo/test_grid_verb.cpp: its GridSession case, RASTER GRID through a
//! session and the MCP server. The file's GridVerb cases are the verb's
//! (step 6's R).

mod grid_session {
    use katana_app::Captured;

    use crate::geo_fixture::holds;
    use crate::mcp_client::{Client, commands, failed, text_of};

    #[test]
    fn raster_grid_runs_through_the_session_and_the_mcp_server() {
        let mut client = Client::new();
        // Three points given a height of 100 by MODIFY, as a person would.
        for line in ["POINT 0,0", "POINT 20,0", "POINT 0,20"] {
            assert!(client.session.run(line, &mut Captured::default()), "{line}");
        }
        assert!(client.session.run(
            "MODIFY DRAWING WHERE TYPE=point SET PROP=elevation:100",
            &mut Captured::default()
        ));
        let result = client.call(
            "katana_run_commands",
            commands(&["RASTER GRID DRAWING cell=5 NAME ground"]),
        );
        assert!(!failed(&result), "{}", result.dump());
        let text = text_of(&result);
        holds(&text, "grid method=linear");
        holds(&text, "target=reference id=1 name=ground");
        let mut captured = Captured::default();
        assert!(client.session.run(
            "RASTER GRID DRAWING cell=5 NAME ground PREVIEW",
            &mut captured
        ));
        holds(&captured.out, "preview valid=yes");
        assert!(
            !client
                .session
                .run("RASTER GRID DRAWING method=kriging", &mut captured)
        );
        holds(&captured.err, "error: InvalidArgument");
    }
}
