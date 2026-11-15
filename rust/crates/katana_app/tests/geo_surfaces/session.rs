//! tests/app/test_session.cpp: the session's case that keeps the reference
//! layers through an OPEN of a polyline and lets them go on NEW. The case of
//! a file named like an id is in tests/geo_surfaces_process (it changes the
//! working folder); the other Session cases are S1's (tests/session) and B's.

mod session {
    use katana_app::{Captured, Session};

    use crate::app_support::{contains, run};
    use crate::geo_fixture::shared_data;
    use crate::surface_support::{quoted, samples};

    #[test]
    fn open_of_a_polyline_keeps_the_reference_layers_and_new_takes_them_off() {
        // OPEN #1 and OPEN SELECTION share the verb with OPEN of a project
        // but open a polyline for editing: the drawing stays, and so do the
        // reference layers worked on top of it. NEW replaces the drawing, and
        // takes them.
        let _shared = shared_data();
        let mut session = Session::new(None, &mut Captured::default());
        let terrain = quoted(&format!("{}/terrain.asc", samples()));
        assert!(run(&mut session, format!("IMPORT {terrain}")).ok);
        assert!(contains(
            &run(&mut session, "REFS").out,
            "references rasters=1 "
        ));
        assert!(run(&mut session, "PLINE 0,0 10,0 10,10 CLOSE").ok);

        let by_id = run(&mut session, "OPEN #1");
        assert!(by_id.ok, "{}", by_id.err);
        assert!(
            contains(&by_id.out, "kind=polyline"),
            "OPEN #1 did not open the polyline"
        );
        assert!(contains(
            &run(&mut session, "REFS").out,
            "references rasters=1 "
        ));

        assert!(run(&mut session, "SELECT #1").ok);
        let by_selection = run(&mut session, "OPEN SELECTION");
        assert!(by_selection.ok, "{}", by_selection.err);
        assert!(contains(&by_selection.out, "kind=polyline"));
        assert!(contains(
            &run(&mut session, "REFS").out,
            "references rasters=1 "
        ));
        // A later line that reads the raster still finds it.
        assert!(run(&mut session, "RASTER SAMPLE RASTER 1 AT 50,50").ok);

        assert!(run(&mut session, "NEW").ok);
        assert!(contains(
            &run(&mut session, "REFS").out,
            "references rasters=0 "
        ));
    }
}
