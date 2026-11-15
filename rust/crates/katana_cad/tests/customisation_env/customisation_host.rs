//! The cases of tests/cad/customisation/test_customisation_host.cpp that set the seam in the
//! environment: what built_in_customisation reads, with the variable unset, `none`, a file, a
//! file that does not read, and the helper that puts it back.

use katana_cad::customisation_host::{
    BUILT_IN_CUSTOMISATION_VARIABLE, built_in_customisation, compiled_in_customisation,
};
use katana_cad::customisation_record::builtin_renames_default;

use crate::customisation_support::{SMALL, Scratch, fnv1a, same};
use crate::{ScopedSeam, environment};

mod customisation_host {
    use super::*;

    #[test]
    fn with_the_seam_unset_the_built_in_is_the_compiled_in_one() {
        let held = environment();
        let _seam = ScopedSeam::new(&held, None);
        let built_in = built_in_customisation();
        let compiled_in = compiled_in_customisation();
        assert!(
            same(&built_in.customisation, &compiled_in.customisation),
            "the very same one, shared"
        );
        assert_eq!(built_in.digest, compiled_in.digest);
        assert_eq!(built_in.problem, compiled_in.problem);
    }

    #[test]
    fn the_seam_says_none_and_there_is_no_built_in_whatever_is_compiled_in() {
        let held = environment();
        for none in ["none", "NONE", "None"] {
            let _seam = ScopedSeam::text(&held, none);
            let built_in = built_in_customisation();
            assert!(built_in.customisation.is_none(), "{none}");
            assert!(built_in.digest.is_empty(), "{none}");
            assert!(
                built_in.problem.is_empty(),
                "asked for, so not a problem: {none}"
            );
        }
    }

    #[test]
    fn the_seam_names_a_file_and_that_file_is_the_built_in() {
        let held = environment();
        let scratch = Scratch::new("env-seam-file");
        let path = scratch.write("small.customisation.json", SMALL);
        let _seam = ScopedSeam::new(&held, Some(path.as_os_str()));

        let built_in = built_in_customisation();
        let customisation = built_in.customisation.as_ref().expect(&built_in.problem);
        assert!(built_in.problem.is_empty());
        assert_eq!(customisation.name, "Small Built In");
        assert_eq!(customisation.library.size(), 2);
        assert_eq!(customisation.map.size(), 2);
        assert_eq!(built_in.digest, fnv1a(SMALL));
    }

    #[test]
    fn a_file_the_seam_names_that_does_not_read_gives_no_built_in_and_says_why() {
        let held = environment();
        let scratch = Scratch::new("env-seam-unread");
        // Not there; and there, but not a customisation.
        let nowhere = scratch.root.join("nowhere.json");
        let other = scratch.write("lines.4d", "worldstyle \"X\" { move 0 0 }\n");
        for path in [&nowhere, &other] {
            let _seam = ScopedSeam::new(&held, Some(path.as_os_str()));
            let built_in = built_in_customisation();
            // None - and never the compiled-in one in its place: a test that
            // named a fixture and was given something else would pass on it.
            assert!(built_in.customisation.is_none(), "{}", path.display());
            assert!(built_in.digest.is_empty(), "{}", path.display());
            assert!(
                built_in.problem.contains(BUILT_IN_CUSTOMISATION_VARIABLE),
                "{}",
                built_in.problem
            );
            let file_name = path
                .file_name()
                .and_then(|name| name.to_str())
                .expect("the scratch names are text");
            assert!(built_in.problem.contains(file_name), "{}", built_in.problem);
        }
    }

    #[test]
    fn the_seam_is_put_back_as_it_was_when_a_test_ends() {
        // The helper these tests lean on, checked: whatever the variable was when
        // the test began - set or not - it is that again afterwards.
        let held = environment();
        let before = std::env::var_os(BUILT_IN_CUSTOMISATION_VARIABLE);
        {
            let seam = ScopedSeam::text(&held, "none");
            assert_eq!(
                std::env::var_os(BUILT_IN_CUSTOMISATION_VARIABLE).as_deref(),
                Some(std::ffi::OsStr::new("none"))
            );
            assert_eq!(seam.before(), before.as_deref());
        }
        assert_eq!(std::env::var_os(BUILT_IN_CUSTOMISATION_VARIABLE), before);
    }

    #[test]
    fn the_renames_made_with_no_name_answer_with_nsw_whatever_the_seam_says() {
        // builtin_renames with no name is the table the Document answers with,
        // and answers with NSW, the customisation the early file names belonged
        // to: not the built-in of the run, which the seam changes, and not the
        // default compiled in. With the seam naming the small fixture, the table
        // must still not answer with "Small Built In" - nor with the default's
        // name when this build compiled in one.
        let held = environment();
        let scratch = Scratch::new("env-renames-and-the-seam");
        let path = scratch.write("small.customisation.json", SMALL);
        let _seam = ScopedSeam::new(&held, Some(path.as_os_str()));
        assert!(built_in_customisation().customisation.is_some());

        let compiled_in = compiled_in_customisation();
        assert!(
            !compiled_in
                .customisation
                .as_ref()
                .is_some_and(|customisation| customisation.name == "Small Built In"),
            "this build compiled in the test's own fixture name"
        );
        // Eight earlier names in two sets of four (customisation_record).
        let renames = builtin_renames_default();
        assert_eq!(renames.len(), 8);
        for rename in &renames {
            assert_eq!(rename.now, "NSW");
        }
    }
}
