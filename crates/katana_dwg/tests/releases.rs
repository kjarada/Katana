//! The DWG releases written and the words that name them (docs/dwg.md, "Releases").

use katana_core::error::ErrorCode;
use katana_dwg::DwgRelease;
use katana_dxf::release_name;

mod dwg_release {
    use super::*;

    #[test]
    fn each_release_written_names_the_version_string_the_dxf_table_gives_it() {
        // katana_dxf's table of $ACADVER values is the independent list: AC1015 is R2000, AC1032
        // R2018, as Autodesk's DXF references name them.
        for release in DwgRelease::ALL {
            assert_eq!(release_name(release.code()), release.name());
        }
    }

    #[test]
    fn the_releases_run_oldest_first_and_r2018_is_the_default() {
        let names: Vec<&str> = DwgRelease::ALL.iter().map(|release| release.name()).collect();
        assert_eq!(names, ["R2000", "R2004", "R2007", "R2010", "R2013", "R2018"]);
        assert!(DwgRelease::ALL.windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!(DwgRelease::DEFAULT, DwgRelease::R2018);
        assert_eq!(DwgRelease::default(), DwgRelease::DEFAULT);
    }

    #[test]
    fn true_colour_comes_with_r2004_and_unicode_with_r2007() {
        // R2004 brought the true colour to the DWG, R2007 its UTF-16 strings.
        assert!(!DwgRelease::R2000.has_true_colour());
        assert!(DwgRelease::R2004.has_true_colour());
        assert!(!DwgRelease::R2004.is_unicode());
        assert!(DwgRelease::R2007.is_unicode());
    }

    #[test]
    fn a_release_is_named_by_its_name_its_year_or_its_version_string_in_any_case() {
        for (word, release) in [
            ("R2018", DwgRelease::R2018),
            ("r2013", DwgRelease::R2013),
            ("2010", DwgRelease::R2010),
            ("AC1021", DwgRelease::R2007),
            ("ac1018", DwgRelease::R2004),
            (" R2000 ", DwgRelease::R2000),
        ] {
            match DwgRelease::parse(word) {
                Ok(parsed) => assert_eq!(parsed, release, "{word}"),
                Err(error) => panic!("{word}: {}", error.describe()),
            }
        }
    }

    #[test]
    fn a_release_read_and_not_written_is_refused_by_name() {
        for (word, name) in [("R14", "R14"), ("AC1012", "R13"), ("r12", "R12")] {
            let error = DwgRelease::parse(word).expect_err("read, not written");
            assert_eq!(error.code, ErrorCode::Unsupported, "{word}");
            assert!(error.message.starts_with(&format!("{name} DWG")), "{}", error.message);
        }
    }

    #[test]
    fn a_word_that_names_no_release_is_refused_with_the_releases_written() {
        for word in ["R2019", "AC1033", "", "newest"] {
            let error = DwgRelease::parse(word).expect_err("no release");
            assert_eq!(error.code, ErrorCode::InvalidArgument, "{word:?}");
            assert!(
                error.message.contains("R2000, R2004, R2007, R2010, R2013, R2018"),
                "{}",
                error.message
            );
        }
    }
}
