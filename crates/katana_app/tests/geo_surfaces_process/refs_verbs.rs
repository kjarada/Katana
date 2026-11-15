//! tests/geo/test_refs_verbs.cpp: the RefsSession case that runs a session in
//! one working folder and opens its project from another.

mod refs_session {
    use katana_app::{Captured, Session};

    use crate::geo_fixture::TempDir;
    use crate::surface_support::{field, quoted, records_of, samples};
    use crate::{InFolder, working_folder};

    #[test]
    fn a_layer_imported_by_a_relative_path_reopens_from_another_folder() {
        // katana_cli run in the data's folder, IMPORT terrain.asc: the record
        // kept "terrain.asc" as typed, and an OPEN from any other folder found
        // nothing there (docs/interop.md said the source was absolute).
        let _lock = working_folder();
        let scratch = TempDir::new("katana refs verbs ", "relative");
        std::fs::create_dir_all(scratch.path().join("data")).expect("the data folder");
        std::fs::create_dir_all(scratch.path().join("elsewhere")).expect("another folder");
        std::fs::copy(
            format!("{}/terrain.asc", samples()),
            scratch.path().join("data").join("terrain.asc"),
        )
        .expect("the sample terrain copied");
        let project = scratch.file("site.katana");
        {
            let _there = InFolder::new(&scratch.path().join("data"));
            let mut session = Session::new(None, &mut Captured::default());
            let mut captured = Captured::default();
            assert!(
                session.run("IMPORT terrain.asc", &mut captured),
                "{}",
                captured.err
            );
            assert!(
                session.run(format!("SAVE {}", quoted(&project)), &mut captured),
                "{}",
                captured.err
            );
        }
        let _elsewhere = InFolder::new(&scratch.path().join("elsewhere"));
        let mut reopened = Session::new(None, &mut Captured::default());
        let mut captured = Captured::default();
        assert!(
            reopened.run(format!("OPEN {}", quoted(&project)), &mut captured),
            "{}",
            captured.err
        );
        let restored = records_of(&captured.out, "restored");
        assert_eq!(restored.len(), 1, "{}", captured.out);
        assert_eq!(field(&restored[0], "layers"), "1", "{}", captured.out);
        assert_eq!(field(&restored[0], "missing"), "0", "{}", captured.out);
    }
}
