//! tests/geo/test_vector_fidelity.cpp: its one case that runs EXPORT through
//! a Session - GDAL's own warnings reach the reply. The file's other cases
//! call interop's import and export, and are interop's (step 5).

mod vector_fidelity {
    use katana_app::{Captured, Session};

    use crate::geo_fixture::{TempDir, holds};

    #[test]
    fn warnings_reach_the_reply() {
        // A shapefile's field names are ten characters at most, and GDAL says
        // so when it shortens one - to a handler that used to drop it.
        let dir = TempDir::new("katana-fidelity-", "warnings");
        let file = dir.file("pegs.shp");
        let mut captured = Captured::default();
        let mut session = Session::new(None, &mut captured);
        assert!(session.run("POINT 1,2", &mut captured));
        assert!(session.run("SELECT ALL", &mut captured));
        assert!(
            session.run("PROP SET surveyed_by_party Smith text", &mut captured),
            "{}",
            captured.err
        );
        let before = captured.out.len();
        assert!(
            session.run(format!("EXPORT \"{file}\""), &mut captured),
            "{}",
            captured.err
        );
        let reply = &captured.out[before..];
        // GDAL's own words, as its shapefile writer says them.
        holds(reply, "'surveyed_by_party' to 'surveyed_b'");
    }
}
