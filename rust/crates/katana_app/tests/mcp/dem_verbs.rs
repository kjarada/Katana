//! tests/geo/test_dem_verbs.cpp: its DemSession cases, the DEM tools through
//! a session and the MCP server, as katana_cli and katana_mcp run them. The
//! file's DemVerbs cases are the verbs' (step 6's R).

mod dem_session {
    use katana_app::geo::own_scratch;

    use crate::geo_fixture::{geo_data, holds, shared_data};
    use crate::mcp_client::{Client, commands, failed, text_of};

    // KATANA_GEO_TEST_DATA/plane.asc.
    fn plane() -> String {
        format!("{}/plane.asc", geo_data())
    }

    #[test]
    fn the_dem_tools_run_through_the_session_and_the_mcp_server() {
        let _shared = shared_data();
        let mut client = Client::new();
        let clip = format!("RASTER CLIP FILE \"{}\" AREA 0,0,20,15 NAME west", plane());
        let result = client.call(
            "katana_run_commands",
            commands(&[&clip, "RASTER FOOTPRINT RASTER west"]),
        );
        assert!(!failed(&result), "{}", result.dump());
        let text = text_of(&result);
        holds(&text, "name=west raster=20x15");
        holds(&text, "layer=gis/footprint created=1");
    }

    // A drawing with no project keeps its derived rasters in a scratch
    // folder, and a result never replaces a file already there. While the
    // folder was one per process, a second Session in the same process - the
    // geo tests run dozens - found the first one's `west` there, and its own
    // came back as `west-2`: a name not asked for, and a reply that depended
    // on which tests ran before. Before that the folder was named by the
    // process id alone, and Windows gives a new process the id of one that has
    // ended, so a later run found an earlier one's files
    // (docs/geoprocessing.md, "Derived rasters").
    #[test]
    fn a_raster_another_session_made_does_not_rename_the_result() {
        let _shared = shared_data();
        let process = std::process::id();
        // Each front end has a folder of its own, inside one of the process's
        // named by its id and when it started.
        let one = own_scratch();
        let two = own_scratch();
        assert_ne!(one, two);
        assert_eq!(one.parent(), two.parent());
        assert!(
            one.parent()
                .and_then(|parent| parent.file_name())
                .is_some_and(|name| name.to_string_lossy().starts_with(&format!("{process}-"))),
            "{}",
            one.display()
        );

        // A server of its own over the session, for each, as the C++ makes
        // one in clipWest.
        let clip_west = |client: &mut Client| -> String {
            let clip = format!("RASTER CLIP FILE \"{}\" AREA 0,0,20,15 NAME west", plane());
            let result = client.call("katana_run_commands", commands(&[&clip]));
            assert!(!failed(&result), "{}", result.dump());
            text_of(&result)
        };
        let mut first = Client::new();
        let mut second = Client::new();
        let made = clip_west(&mut first);
        let again = clip_west(&mut second);
        holds(&made, "name=west raster=20x15");
        holds(&again, "name=west raster=20x15");
    }
}
