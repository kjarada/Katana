//! tests/app/test_session.cpp: the session katana_cli and katana_mcp share,
//! as katana_cli runs a line - its IMPORT placements, which the executor's
//! IMPORT answers. The file's other cases are the session's targets'
//! (tests/session, tests/session_process) and agent A's (the reference
//! layers). IMPORT replies in records (docs/interop.md), so its checks read
//! fields.

mod session {
    use katana_app::{Captured, Session};

    use crate::app_support::{Printed, ScratchDirectory, contains, generic, run};
    use crate::double_eq;
    use crate::geo_fixture::{checkout, shared_data};

    /// A session of no program (the C++'s nullptr): it starts empty and reads
    /// no variable.
    fn of_no_program() -> Session {
        let mut started = Captured::default();
        Session::new(None, &mut started)
    }

    /// A file of samples/gis, quoted for a line.
    fn sample(name: &str) -> String {
        format!("\"{}\"", generic(&checkout("samples/gis").join(name)))
    }

    #[test]
    fn a_quoted_import_ending_in_local_moves_the_data_to_the_origin() {
        let _shared = shared_data();
        // katana_import sends IMPORT "path" LOCAL. LOCAL was taken off and the
        // quotes left on, so every such import looked for a file named with
        // its quotes. The folder has a blank in its name, so the quotes are
        // needed. samples/gis/parcels.geojson spans (180, 0) to (365, 165),
        // so moved as one piece to put its lower-left corner at 0,0 it spans
        // (0, 0) to (185, 165).
        let scratch = ScratchDirectory::new("import local");
        let copy = scratch.path.join("site parcels.geojson");
        std::fs::copy(checkout("samples/gis").join("parcels.geojson"), &copy)
            .expect("the sample is copied");
        let mut session = of_no_program();
        let imported: Printed = run(&mut session, format!("IMPORT \"{}\" LOCAL", generic(&copy)));
        assert!(imported.ok, "{}", imported.err);
        assert!(
            contains(&imported.out, " bounds=0,0,185,165 "),
            "{}",
            imported.out
        );
        let bounds = session.document().model().entities.bounds();
        double_eq(bounds.min.x, 0.0);
        double_eq(bounds.min.y, 0.0);
        double_eq(bounds.max.x, 185.0);
        double_eq(bounds.max.y, 165.0);
    }

    #[test]
    fn alongside_and_an_offset_place_the_data_and_say_the_move() {
        let _shared = shared_data();
        // parcels.geojson spans (180, 0) to (365, 165). ALONGSIDE puts that
        // corner on the drawing's, the rectangle's (1000, 2000);
        // OFFSET=10,-20.5 moves it to (190, -20.5).
        let mut session = of_no_program();
        let parcels = sample("parcels.geojson");
        assert!(run(&mut session, "RECT 1000,2000 1010,2005").ok);
        let along = run(&mut session, format!("IMPORT {parcels} ALONGSIDE"));
        assert!(along.ok, "{}", along.err);
        assert!(
            contains(
                &along.out,
                "placed placement=alongside east=820 north=2000 text=\"ALONGSIDE: moved as one \
                 piece by 820.000,2000.000"
            ),
            "{}",
            along.out
        );
        assert!(
            contains(&along.out, " bounds=1000,2000,1185,2165 "),
            "{}",
            along.out
        );

        let mut offset = of_no_program();
        let moved = run(&mut offset, format!("IMPORT {parcels} offset=10,-20.5"));
        assert!(moved.ok, "{}", moved.err);
        let bounds = offset.document().model().entities.bounds();
        double_eq(bounds.min.x, 190.0);
        double_eq(bounds.min.y, -20.5);
        double_eq(bounds.max.x, 375.0);
        double_eq(bounds.max.y, 144.5);
    }

    #[test]
    fn a_placement_is_refused_by_name_for_raster_and_a_bad_offset_for_any_file() {
        let _shared = shared_data();
        let mut session = of_no_program();
        let terrain = sample("terrain.asc");
        let raster = run(&mut session, format!("IMPORT {terrain} OFFSET=1,2"));
        assert!(!raster.ok);
        assert!(
            contains(
                &raster.err,
                "OFFSET is not supported for rasters and point clouds"
            ),
            "{}",
            raster.err
        );
        let bad = run(&mut session, format!("IMPORT {terrain} OFFSET=1"));
        assert!(!bad.ok);
        assert!(
            contains(&bad.err, "OFFSET= takes the east and north"),
            "{}",
            bad.err
        );
        assert!(session.document().model().entities.empty());
    }
}
