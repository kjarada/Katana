//! tests/app/test_session.cpp: the Session case that runs INFO beside a file
//! named like an entity's id.

mod session {
    use katana_app::{Captured, Session};

    use crate::app_support::{ScratchDirectory, contains, run};
    use crate::{InFolder, working_folder};

    #[test]
    fn info_of_a_file_named_like_an_id_still_reads_the_file() {
        // The id is the interpreter's only when no file of that name exists:
        // a file called 12 beside the session is described (and, being no GIS
        // file, refused as one), never looked up as entity 12.
        let _lock = working_folder();
        let scratch = ScratchDirectory::new("info-file");
        scratch.file("12", "not a GIS file\n");
        let info = {
            let _there = InFolder::new(&scratch.path);
            let mut session = Session::new(None, &mut Captured::default());
            run(&mut session, "INFO 12")
        };
        assert!(!info.ok);
        assert!(
            !contains(&info.err, "entity does not exist"),
            "{}",
            info.err
        );
        assert!(info.err.starts_with("error: "), "{}", info.err);
    }
}
