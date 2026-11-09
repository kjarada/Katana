//! The case of tests/cad/customisation/test_customisation_verbs.cpp that changes the working
//! folder: files called exactly as two of CUSTOMISE's keywords, named from that folder.

use std::fs;
use std::path::Path;

use katana_core::ErrorCode;
use katana_core::path_text::path_from_utf8;

use crate::customisation_support::Scratch;
use crate::verbs_support::{Bare, SITE, contents_of, field, parsed, record};
use crate::{WorkingFolder, working_folder_lock};

// The C++'s fs::equivalent: whether two paths name the one file; false when
// either cannot be resolved.
fn equivalent(left: &Path, right: &Path) -> bool {
    match (fs::canonicalize(left), fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

mod customisation_verbs {
    use super::*;

    #[test]
    fn a_keyword_is_the_whole_first_word_and_a_file_so_named_is_given_with_its_directory() {
        // Files called exactly as two of the keywords, in the working directory.
        let held = working_folder_lock();
        let scratch = Scratch::new("keywords");
        scratch.write("json", SITE);
        scratch.write("replace", SITE);
        let _restore = WorkingFolder::new(&held, &scratch.root);

        let mut session = Bare::default();
        // The bare words are the keywords, in any case: the report, and a
        // REPLACE with no file.
        assert!(
            session.ok("CUSTOMISE json").starts_with("{\n"),
            "the report, as JSON"
        );
        assert_eq!(session.document.survey_map().size(), 0);
        assert_eq!(
            session.refused("CUSTOMISE Replace").message,
            "usage: CUSTOMISE [REPLACE] <file> [<file>...]"
        );
        // With its directory, each is the file. (The command line removes quotes
        // before a verb sees its words, so a quoted "json" is the keyword too.)
        assert!(session.ok("CUSTOMISE \"json\"").starts_with("{\n"));
        let loaded = session.ok("CUSTOMISE ./json");
        assert_eq!(field(&record(&loaded, "loaded", 0), "name"), "Site");
        assert_eq!(session.document.survey_map().size(), 3);
        // After REPLACE the word is a file whatever it says. (Compared as files:
        // the working directory may be spelled another way than the folder was.)
        let replaced = session.ok("CUSTOMISE REPLACE replace");
        let named = field(&record(&replaced, "loaded", 0), "file");
        assert!(
            equivalent(&path_from_utf8(&named), &scratch.root.join("replace")),
            "{named}"
        );
        // A word that is no keyword is a file, and is refused as one that is
        // not there: LOAD is not a word of this verb.
        assert_eq!(
            session.refused("CUSTOMISE LOAD ./json").code,
            ErrorCode::NotFound
        );

        // EXPORT's first word is the file, and one of EXPORT's own words there
        // is a line whose file was left out: CUSTOMISE EXPORT CODES once wrote
        // the whole session into a file called CODES. With its directory, a file
        // so called is the file. (The session holds Site: 3 rules.)
        for word in ["CODES", "linestyles", "Symbols", "NAME", "only"] {
            let error = session.refused(&format!("CUSTOMISE EXPORT {word}"));
            assert_eq!(error.code, ErrorCode::InvalidArgument, "{word}");
            assert!(
                error.message.starts_with("usage: CUSTOMISE EXPORT <file> "),
                "{}",
                error.message
            );
            assert!(
                error
                    .message
                    .ends_with(&format!("is given with its directory (./{word})")),
                "{}",
                error.message
            );
            assert_eq!(error.context, word);
            assert!(!scratch.root.join(word).exists(), "{word}");
        }
        let exported = session.ok("CUSTOMISE EXPORT ./CODES");
        assert_eq!(field(&record(&exported, "exported", 0), "rules"), "3");
        assert_eq!(
            parsed(&contents_of(&scratch.root.join("CODES"))).map.size(),
            3
        );
    }
}
