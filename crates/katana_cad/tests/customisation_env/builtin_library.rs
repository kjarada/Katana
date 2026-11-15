//! The cases of tests/cad/customisation/test_builtin_library.cpp that change the working folder
//! or set the seam in the environment: a file called like a built-in, a mistyped name, and the
//! name form read from the environment.

use katana_cad::customisation_host::{
    BUILT_IN_CUSTOMISATION_VARIABLE, BuiltInCustomisation, built_in_customisation,
    built_in_customisation_for, built_ins, compiled_in_customisation, find_built_in,
};

use crate::customisation_support::{BETA, Scratch, same, two_entries};
use crate::{ScopedSeam, WorkingFolder, environment};

mod built_in_library {
    use super::*;

    #[test]
    fn a_file_called_like_a_built_in_in_the_working_folder_is_given_with_its_folder() {
        // `./Alpha` has a separator, so it is a file; `Alpha` is the built-in.
        let held = environment();
        let scratch = Scratch::new("env-dot-slash");
        scratch.write("Alpha", BETA);
        let entries = two_entries();
        let (by_name, by_file) = {
            let _folder = WorkingFolder::new(&held, &scratch.root);
            (
                built_in_customisation_for("Alpha", &BuiltInCustomisation::default(), &entries),
                built_in_customisation_for("./Alpha", &BuiltInCustomisation::default(), &entries),
            )
        };
        let named = by_name.customisation.as_ref().expect(&by_name.problem);
        assert_eq!(
            named.name, "Alpha",
            "the built-in, the file beside it unread"
        );
        let read = by_file.customisation.as_ref().expect(&by_file.problem);
        assert_eq!(read.name, "Beta Two", "the file");
    }

    #[test]
    fn a_word_that_is_no_built_in_is_read_as_a_file_and_the_reason_lists_the_names_there_are() {
        let held = environment();
        let entries = two_entries();
        let scratch = Scratch::new("env-mistyped");
        let built_in = {
            // A folder with no file called Alfa.
            let _folder = WorkingFolder::new(&held, &scratch.root);
            built_in_customisation_for("Alfa", &entries[0].built_in, &entries)
        };
        // None - and never the default in its place.
        assert!(built_in.customisation.is_none());
        assert!(
            built_in.problem.contains(BUILT_IN_CUSTOMISATION_VARIABLE),
            "{}",
            built_in.problem
        );
        assert!(built_in.problem.contains("Alfa"), "{}", built_in.problem);
        assert!(
            built_in.problem.contains("\"Alpha\", \"Beta Two\""),
            "{}",
            built_in.problem
        );
        assert!(built_in.problem.contains("./Alfa"), "{}", built_in.problem);

        // With no names to offer there is nothing to list, and it reads as it
        // did.
        let alone = built_in_customisation_for("Alfa", &BuiltInCustomisation::default(), &[]);
        assert!(!alone.problem.contains("either"), "{}", alone.problem);
    }

    #[test]
    fn the_seam_in_the_environment_names_a_compiled_in_built_in_in_any_letter_case() {
        let Some(nsw) = find_built_in(built_ins(), "NSW") else {
            eprintln!("this build has no NSW customisation compiled in");
            return;
        };
        let held = environment();
        for spelling in ["NSW", "nsw", "Nsw"] {
            let _seam = ScopedSeam::text(&held, spelling);
            let built_in = built_in_customisation();
            assert!(
                built_in.customisation.is_some(),
                "{spelling}: {}",
                built_in.problem
            );
            assert!(
                same(&built_in.customisation, &nsw.built_in.customisation),
                "{spelling}"
            );
            assert_eq!(built_in.digest, nsw.built_in.digest, "{spelling}");
        }
        // The compiled-in default is untouched by what the seam says.
        let _seam = ScopedSeam::text(&held, "NSW");
        assert!(!same(
            &compiled_in_customisation().customisation,
            &nsw.built_in.customisation
        ));
    }

    #[test]
    fn the_seam_in_the_environment_names_the_default_by_name_too() {
        let entries = built_ins();
        if entries.is_empty() || compiled_in_customisation().customisation.is_none() {
            eprintln!("this build has no default built-in compiled in");
            return;
        }
        let held = environment();
        let _seam = ScopedSeam::text(&held, &entries[0].name);
        let built_in = built_in_customisation();
        assert!(same(
            &built_in.customisation,
            &compiled_in_customisation().customisation
        ));
    }
}
