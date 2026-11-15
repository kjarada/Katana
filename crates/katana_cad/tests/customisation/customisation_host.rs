//! Where a session's customisation comes from when it starts (customisation_host): reading a
//! file, the built-in and the seam that says what it is for one run, and start_customisation's
//! choice between the kept file, the built-in and nothing
//! (tests/cad/customisation/test_customisation_host.cpp).
//!
//! "The built-in" in these tests is the small customisation of the shared support, handed over by
//! the test as a host hands one over. Nothing here reads the customisation a build may have
//! compiled in, except to check that asking for it is safe whether or not there is one - so the
//! suite is the same on a machine whose build has it and on one whose build has not. The cases
//! that set the seam in the environment are in the target tests/customisation_env.

use std::fs;
use std::sync::{Arc, Mutex};

use katana_cad::customisation_host::{
    BUILT_IN_CUSTOMISATION_VARIABLE, BuiltInCustomisation, BuiltInEntry, CustomisationHost,
    built_in_customisation_for, compiled_in_customisation, install_built_in_customisation,
    read_customisation_file, start_customisation,
};
use katana_cad::customisation_record::CustomisationSource;
use katana_cad::customisation_report::{customisation_json, customisation_summary};
use katana_cad::customisation_state::{CustomisationOrigin, CustomisationState};
use katana_cad::{Document, DocumentChange};
use katana_core::ErrorCode;
use katana_core::path_text::{path_from_utf8, path_to_utf8};
use katana_entity::customisation::{
    customisation_from_json, customisation_to_json, validate_customisation_name,
};
use katana_entity::{
    Customisation, CustomisationAutomation, CustomisationWriteOptions, LineworkCodes,
};

use crate::customisation_support::{
    SMALL, Scratch, built_in_from, fnv1a, path_text, same, stand_in,
};

// A user's own customisation: one rule, and what it was made from - the built-in of that name, in
// the edition of that digest - when `based_on_digest` gives one.
fn kept_text(based_on_digest: &str, based_on_name: &str) -> String {
    let mut text =
        String::from(r#"{"format": "katana-customisation", "version": 1, "name": "Mine","#);
    if !based_on_digest.is_empty() {
        text.push_str(&format!(
            r#" "basedOn": {{"name": "{based_on_name}", "digest": "{based_on_digest}"}},"#
        ));
    }
    text.push_str(r#" "codes": [{"key": "ZZ*", "sets": "feature", "layer": "MINE"}]}"#);
    text
}

fn kept(based_on_digest: &str) -> String {
    kept_text(based_on_digest, "Small Built In")
}

// A second built-in for a host to offer, an alternate to the small one: 1 definition, 1 rule over
// 1 key.
const ALTERNATE: &str = r#"{
  "format": "katana-customisation", "version": 1, "name": "Alternate One",
  "linestyles": [{"name": "ALT Line", "strokes": [["move", 0, 0], ["draw", 3, 0]]}],
  "codes": [{"key": "AL*", "sets": "feature", "layer": "ALTERNATE", "linestyle": "ALT Line"}]
}"#;

// `text` as an entry of the list a host offers.
fn entry_of(text: &str) -> BuiltInEntry {
    let built_in = built_in_from(text);
    BuiltInEntry {
        name: built_in
            .customisation
            .as_ref()
            .map_or_else(String::new, |customisation| customisation.name.clone()),
        built_in,
        ..BuiltInEntry::default()
    }
}

fn host_of(built_in: BuiltInCustomisation) -> CustomisationHost {
    CustomisationHost {
        built_in,
        ..CustomisationHost::default()
    }
}

// ---- reading a file --------------------------------------------------------------------------

mod customisation_host {
    use super::*;

    #[test]
    fn reading_a_file_gives_the_customisation_and_the_digest_of_its_bytes() {
        let scratch = Scratch::new("host-read");
        let path = scratch.write("small.customisation.json", SMALL);

        let file =
            read_customisation_file(&path).unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(file.customisation.name, "Small Built In");
        assert_eq!(file.customisation.library.size(), 2);
        assert_eq!(file.customisation.map.size(), 2);
        // Which array a definition sits in says whether it is listed as a
        // symbol, and the customisation it sits in is where it is from.
        let peg = file
            .customisation
            .library
            .find("TEST Peg")
            .expect("the peg");
        assert!(peg.symbol);
        let fence = file
            .customisation
            .library
            .find("TEST Fence")
            .expect("the fence");
        assert!(!fence.symbol);
        assert_eq!(fence.source, "Small Built In");
        // The digest is of the bytes as they are on disk.
        assert_eq!(file.digest, fnv1a(SMALL));
        assert_eq!(file.digest.len(), 16);
    }

    #[test]
    fn a_file_that_is_not_there_or_is_no_customisation_is_refused_naming_the_file() {
        let scratch = Scratch::new("host-refused");

        let missing = read_customisation_file(&scratch.root.join("nowhere.json"))
            .expect_err("nothing is there");
        assert_eq!(missing.code, ErrorCode::NotFound);
        assert!(
            missing.context.contains("nowhere.json"),
            "{}",
            missing.describe()
        );

        let directory = read_customisation_file(&scratch.root).expect_err("a folder");
        assert_eq!(directory.code, ErrorCode::NotFound);

        // Another program's format - here a style library's own text - is not
        // read, and is said to be what it is not.
        let library = scratch.write("lines.4d", "worldstyle \"X\" { move 0 0 draw 1 0 }\n");
        let other = read_customisation_file(&library).expect_err("another format");
        assert_eq!(other.code, ErrorCode::ParseFailure);
        assert_eq!(other.message, "not a Katana customisation file");
        assert!(
            other.context.starts_with(&path_text(&library)),
            "the file first: {}",
            other.describe()
        );

        // An empty file is not one either, and is refused rather than read as a
        // customisation of nothing.
        let empty =
            read_customisation_file(&scratch.write("empty.json", "")).expect_err("an empty file");
        assert_eq!(empty.code, ErrorCode::ParseFailure);

        // A customisation with a member the format does not know: which FILE,
        // then which entry.
        let typo = scratch.write(
            "typo.json",
            r#"{"format": "katana-customisation", "version": 1, "name": "T",
 "codes": [{"key": "WM*", "sets": "feature", "linesytle": "TEST Water Main"}]}"#,
        );
        let strict = read_customisation_file(&typo).expect_err("a member it does not know");
        assert_eq!(strict.code, ErrorCode::ParseFailure);
        assert!(
            strict.context.starts_with(&path_text(&typo)),
            "{}",
            strict.describe()
        );
        assert!(strict.message.contains("\"WM*\""), "{}", strict.describe());
        assert!(
            strict.message.contains("\"linesytle\""),
            "{}",
            strict.describe()
        );
    }

    // ---- the customisation compiled in, and the seam -----------------------------------------

    #[test]
    fn the_compiled_in_customisation_is_read_or_there_is_none_and_never_one_that_did_not_read() {
        // Whichever this build is. With none: none, and NO PROBLEM - an absent
        // built-in is a state a build may be in, not a fault. With one: it was
        // read, and carries the digest of its bytes. What this build may not be
        // is the third thing: a file compiled in that is no customisation. The
        // program still starts with that, and says so; a release made from such
        // a build would draw plain lines, and this is the test that stops it -
        // the configure checks only that the file is there.
        let compiled_in = compiled_in_customisation();
        match &compiled_in.customisation {
            None => {
                assert!(compiled_in.digest.is_empty());
                assert!(compiled_in.problem.is_empty(), "{}", compiled_in.problem);
            }
            Some(customisation) => {
                assert!(compiled_in.problem.is_empty());
                assert!(validate_customisation_name(&customisation.name).is_ok());
                assert_eq!(compiled_in.digest.len(), 16);
            }
        }
        // Parsed once: the same object every time.
        assert!(std::ptr::eq(compiled_in, compiled_in_customisation()));
    }

    // ---- the rule of the seam, against a built-in that IS compiled in ------------------------
    //
    // The tests in the environment go through it and whatever this build compiled
    // in - the tracked Katana Standard customisation in a default build, nothing in
    // one whose configuration names no file - so "whatever is compiled in" and
    // "never the compiled-in one in its place" could not fail in the second. These
    // give the rule a compiled-in customisation to override in both.

    #[test]
    fn with_nothing_said_the_built_in_is_the_compiled_in_one_as_it_is() {
        let compiled_in = stand_in();
        assert!(compiled_in.customisation.is_some());
        let built_in = built_in_customisation_for("", &compiled_in, &[]);
        assert!(
            same(&built_in.customisation, &compiled_in.customisation),
            "the very same one, shared"
        );
        assert_eq!(
            built_in.customisation.as_ref().map(|c| c.name.as_str()),
            Some("Stand In")
        );
        assert_eq!(built_in.digest, compiled_in.digest);
        assert!(built_in.problem.is_empty());

        // One that did not read is handed on with its reason, not hidden.
        let damaged = BuiltInCustomisation {
            problem: String::from(
                "the customisation compiled into this program is not read: it is damaged",
            ),
            ..BuiltInCustomisation::default()
        };
        let passed = built_in_customisation_for("", &damaged, &[]);
        assert!(passed.customisation.is_none());
        assert_eq!(passed.problem, damaged.problem);
    }

    #[test]
    fn saying_none_gives_no_built_in_though_one_is_compiled_in() {
        let compiled_in = stand_in();
        assert!(compiled_in.customisation.is_some());
        for none in ["none", "NONE", "None"] {
            let built_in = built_in_customisation_for(none, &compiled_in, &[]);
            assert!(built_in.customisation.is_none(), "{none}");
            assert!(built_in.digest.is_empty(), "{none}");
            assert!(
                built_in.problem.is_empty(),
                "asked for, so not a problem: {none}"
            );
        }
    }

    #[test]
    fn a_file_named_takes_the_place_of_the_compiled_in_customisation() {
        let scratch = Scratch::new("host-seam-over-compiled-in");
        let path = scratch.write("small.customisation.json", SMALL);
        let built_in = built_in_customisation_for(path_to_utf8(&path), &stand_in(), &[]);
        let customisation = built_in.customisation.as_ref().expect(&built_in.problem);
        assert_eq!(
            customisation.name, "Small Built In",
            "the file, not the stand-in"
        );
        assert_eq!(built_in.digest, fnv1a(SMALL));
        assert!(built_in.problem.is_empty());
    }

    #[test]
    fn a_file_named_that_does_not_read_gives_none_and_never_the_compiled_in_one() {
        let scratch = Scratch::new("host-seam-unread-over-compiled-in");
        // Not there; and there, but not a customisation.
        let nowhere = scratch.root.join("nowhere.json");
        let other = scratch.write("lines.4d", "worldstyle \"X\" { move 0 0 }\n");
        let compiled_in = stand_in();
        assert!(compiled_in.customisation.is_some());
        for path in [&nowhere, &other] {
            let built_in = built_in_customisation_for(path_to_utf8(path), &compiled_in, &[]);
            // A test that named a fixture and was given the compiled-in
            // customisation instead would pass on it.
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
    fn the_file_is_named_as_text_whether_or_not_its_bytes_are_utf8() {
        let scratch = Scratch::new("host-seam-names");
        // UTF-8, under a directory whose name is not ASCII: "Zürich", the
        // u-umlaut the two bytes C3 BC.
        let zurich = scratch.root.join(path_from_utf8(b"Z\xC3\xBCrich"));
        fs::create_dir_all(&zurich).expect("the folder is made");
        fs::write(zurich.join("small.json"), SMALL).expect("the file is written");
        let named = built_in_customisation_for(
            path_to_utf8(&zurich.join("small.json")),
            &BuiltInCustomisation::default(),
            &[],
        );
        let customisation = named.customisation.as_ref().expect(&named.problem);
        assert_eq!(customisation.name, "Small Built In");

        // Narrow bytes that are NOT UTF-8, which is how an environment gives a
        // name in the ANSI code page: "café" with its é the one byte E9. The
        // C++ names it relative to the scratch folder, made the working folder,
        // so that nothing but the name under test is narrow; the working folder
        // is the process's, and this binary runs its tests at once, so here the
        // name is the scratch folder's own (ASCII on these systems) and the
        // narrow name after it. A file system's names are bytes on Unix alone.
        #[cfg(unix)]
        {
            let mut narrow = path_to_utf8(&scratch.root);
            narrow.extend_from_slice(b"/caf\xE9.customisation.json");
            let absent = built_in_customisation_for(&narrow, &BuiltInCustomisation::default(), &[]);
            // Not there yet: no built-in, and the reason.
            assert!(absent.customisation.is_none());
            assert!(
                absent.problem.contains(BUILT_IN_CUSTOMISATION_VARIABLE),
                "{}",
                absent.problem
            );
            if fs::write(path_from_utf8(&narrow), SMALL).is_err() {
                // A file system that takes only UTF-8 names refuses this one.
                eprintln!("this system makes no file of that narrow name");
                return;
            }
            let present = built_in_customisation_for(narrow, &BuiltInCustomisation::default(), &[]);
            // And it is THE file made under that narrow name.
            let customisation = present.customisation.as_ref().expect(&present.problem);
            assert_eq!(customisation.name, "Small Built In");
        }
    }
}

// ---- what a session starts with --------------------------------------------------------------

mod customisation_start {
    use super::*;

    #[test]
    fn with_no_host_nothing_is_installed_and_that_is_no_problem() {
        let mut document = Document::default();
        let report = start_customisation(&mut document, &CustomisationHost::default());
        assert_eq!(report.installed, CustomisationOrigin::None);
        assert!(report.name.is_empty());
        assert_eq!(report.definitions, 0);
        assert_eq!(report.rules, 0);
        assert!(report.problems.is_empty());
        assert!(!report.kept_from_another_built_in);
        // The Document is the empty one it was.
        assert_eq!(
            document.customisation_state(),
            &CustomisationState::default()
        );
        assert!(document.style_library().empty());
        assert!(document.survey_map().empty());
        assert_eq!(document.customisation_generation(), 0);
    }

    #[test]
    fn installing_a_built_in_that_is_not_there_is_refused_and_the_document_is_left_as_it_was() {
        // The one function a start and CUSTOMISE RESET both install the built-in
        // through. Handed a host's built-in that holds none, it refuses - the
        // verb asks first and never reaches this, a front end calling it may.
        let mut document = Document::default();
        let none =
            install_built_in_customisation(&mut document, &BuiltInCustomisation::default(), true)
                .expect_err("there is none");
        assert_eq!(none.code, ErrorCode::InvalidState);
        assert_eq!(
            none.message,
            "there is no built-in customisation to install"
        );
        assert_eq!(
            document.customisation_state(),
            &CustomisationState::default()
        );
        assert_eq!(document.customisation_generation(), 0);

        // With one, `kept` is the caller's to say: a RESET while a kept file is
        // there installs the built-in as a session the next start would NOT give.
        if let Err(error) =
            install_built_in_customisation(&mut document, &built_in_from(SMALL), false)
        {
            panic!("{}", error.describe());
        }
        let state = document.customisation_state();
        assert_eq!(state.origin, CustomisationOrigin::BuiltIn);
        assert!(!state.kept);
        let based_on = state.based_on.as_ref().expect("based on itself");
        assert_eq!(based_on.name, "Small Built In");
        assert_eq!(based_on.digest, fnv1a(SMALL));
    }

    #[test]
    fn the_built_in_is_installed_when_nothing_is_kept() {
        let scratch = Scratch::new("start-built-in");
        let host = CustomisationHost {
            built_in: built_in_from(SMALL),
            kept_file: scratch.root.join("customisation.json"), // nobody has kept anything
            ..CustomisationHost::default()
        };

        let mut document = Document::default();
        let report = start_customisation(&mut document, &host);

        assert_eq!(report.installed, CustomisationOrigin::BuiltIn);
        assert_eq!(report.name, "Small Built In");
        assert_eq!(report.definitions, 2);
        assert_eq!(report.symbols, 1);
        assert_eq!(report.rules, 2);
        assert_eq!(report.colours, 1);
        assert!(report.problems.is_empty(), "{:?}", report.problems);
        assert!(!report.kept_from_another_built_in);

        let state = document.customisation_state();
        assert_eq!(state.name, "Small Built In");
        assert_eq!(state.origin, CustomisationOrigin::BuiltIn);
        assert!(state.kept, "it is what the next start would give");
        assert_eq!(state.built_in, "Small Built In");
        assert_eq!(
            state.description,
            "Two definitions and two rules, for tests"
        );
        assert_eq!(state.notice, vec![String::from("Written for these tests.")]);
        assert_eq!(state.linework.start, "S");
        assert_eq!(
            state.sources,
            vec![CustomisationSource {
                name: String::from("Small Built In"),
                definitions: true,
                rules: true,
                notice: Vec::new(),
            }]
        );
        // Based on itself, so that a copy kept from it can be told from one made
        // against another edition.
        let based_on = state.based_on.as_ref().expect("based on itself");
        assert_eq!(based_on.name, "Small Built In");
        assert_eq!(based_on.digest, fnv1a(SMALL));

        assert_eq!(document.style_library().size(), 2);
        assert_eq!(
            document.survey_map().lookup("FE07").resolved.model,
            "FENCES"
        );
        // The host's own copy is untouched: it is shared, not handed over.
        let own = host
            .built_in
            .customisation
            .as_ref()
            .expect("the host's built-in");
        assert!(own.based_on.is_none());
        assert!(!document.is_modified());
    }

    // A symbol is what the catalogue offers as one: a definition a survey rule
    // names as its vertex symbol is a symbol though nobody listed it as one and it
    // is not drawn at vertices. The start's count is the report's (CUSTOMISE), not
    // the count of definitions listed as symbols - they were 471 and 472 over the
    // owner's built-in.
    #[test]
    fn the_start_counts_symbols_as_customise_does() {
        // Counted by hand: 2 definitions, both plain linestyles (none listed as a
        // symbol, none at vertices); the one rule names "TEST Mark" as its
        // vertex symbol, so exactly 1 of them is offered as a symbol.
        let text = r#"{
      "format": "katana-customisation", "version": 1, "name": "Evidence",
      "linestyles": [
        {"name": "TEST Mark", "strokes": [["move", 0, 0], ["draw", 1, 0]]},
        {"name": "TEST Wall", "strokes": [["move", 0, 0], ["draw", 2, 0]]}
      ],
      "codes": [
        {"key": "MK*", "sets": "symbol", "symbol": {"name": "TEST Mark", "size": 1}}
      ]
    }"#;
        let scratch = Scratch::new("start-symbol-count");
        let host = CustomisationHost {
            built_in: built_in_from(text),
            kept_file: scratch.root.join("customisation.json"),
            ..CustomisationHost::default()
        };

        let mut document = Document::default();
        let report = start_customisation(&mut document, &host);

        assert_eq!(report.definitions, 2);
        assert_eq!(report.symbols, 1);
        assert_eq!(customisation_summary(&document).symbols, report.symbols);
    }

    #[test]
    fn the_kept_file_is_installed_in_preference_to_the_built_in() {
        let scratch = Scratch::new("start-kept");
        let host = CustomisationHost {
            built_in: built_in_from(SMALL),
            kept_file: scratch.write("customisation.json", &kept(&fnv1a(SMALL))),
            ..CustomisationHost::default()
        };

        let mut document = Document::default();
        let report = start_customisation(&mut document, &host);

        assert_eq!(report.installed, CustomisationOrigin::Kept);
        assert_eq!(report.name, "Mine");
        assert_eq!(report.definitions, 0);
        assert_eq!(report.symbols, 0);
        assert_eq!(report.rules, 1);
        assert_eq!(report.colours, 0);
        assert!(report.problems.is_empty());
        assert!(
            !report.kept_from_another_built_in,
            "made from this very built-in"
        );

        let state = document.customisation_state();
        assert_eq!(state.name, "Mine");
        assert_eq!(state.origin, CustomisationOrigin::Kept);
        assert!(state.kept);
        // The built-in's name is the host's, whatever is installed.
        assert_eq!(state.built_in, "Small Built In");
        assert!(
            document.style_library().empty(),
            "the kept one, not the two merged"
        );
        assert_eq!(document.survey_map().lookup("ZZ1").resolved.model, "MINE");
    }

    #[test]
    fn a_kept_file_made_from_another_edition_of_the_built_in_is_installed_and_said_to_be() {
        let scratch = Scratch::new("start-another-edition");
        let mut host = host_of(built_in_from(SMALL));

        // Made from bytes that are not this built-in's.
        host.kept_file = scratch.write("earlier.json", &kept("0123456789abcdef"));
        let mut earlier = Document::default();
        let moved = start_customisation(&mut earlier, &host);
        assert_eq!(
            moved.installed,
            CustomisationOrigin::Kept,
            "it is still the user's"
        );
        assert!(moved.kept_from_another_built_in);
        assert!(
            moved.problems.is_empty(),
            "something to say, not something wrong"
        );

        // Made from a built-in of ANOTHER NAME: another built-in altogether,
        // whatever its digest is said to be.
        host.kept_file = scratch.write(
            "renamed.json",
            &kept_text(&fnv1a(SMALL), "Another Built In"),
        );
        let mut renamed = Document::default();
        let other = start_customisation(&mut renamed, &host);
        assert_eq!(other.installed, CustomisationOrigin::Kept);
        assert!(other.kept_from_another_built_in);

        // One that does not say what it was made from cannot be said to differ.
        host.kept_file = scratch.write("silent.json", &kept(""));
        let mut silent = Document::default();
        assert!(!start_customisation(&mut silent, &host).kept_from_another_built_in);

        // Nor can any, where this program has no built-in to have moved on.
        let bare = CustomisationHost {
            kept_file: scratch.root.join("earlier.json"),
            ..CustomisationHost::default()
        };
        let mut with_none = Document::default();
        let alone = start_customisation(&mut with_none, &bare);
        assert_eq!(alone.installed, CustomisationOrigin::Kept);
        assert!(!alone.kept_from_another_built_in);
        assert!(with_none.customisation_state().built_in.is_empty());
    }

    #[test]
    fn a_built_in_that_says_what_it_was_made_from_is_still_installed_based_on_itself() {
        // A built-in that was itself exported from a session names the earlier
        // customisation IT was made from. By hand: the session started from it is
        // based on the built-in - its name, the digest of its bytes - and not on
        // that earlier one; so a copy kept from the session, read at the next
        // start, was made from this very built-in, and nothing has moved on.
        // (Left based on the earlier one, the copy carried a digest that is not
        // this built-in's and was reported as made from another, at every start.)
        let name = r#""name": "Small Built In","#;
        let at = SMALL.find(name).expect("the fixture names itself") + name.len();
        let exported = format!(
            "{}{}{}",
            &SMALL[..at],
            r#" "basedOn": {"name": "Earlier Edition", "digest": "0123456789abcdef"},"#,
            &SMALL[at..]
        );

        let scratch = Scratch::new("start-built-in-based-on-another");
        let host = CustomisationHost {
            built_in: built_in_from(&exported),
            kept_file: scratch.root.join("customisation.json"),
            ..CustomisationHost::default()
        };
        let fixture = host.built_in.customisation.as_ref().expect("the built-in");
        let says = fixture.based_on.as_ref().expect("the fixture does say");
        assert_eq!(says.name, "Earlier Edition");

        let mut first = Document::default();
        assert_eq!(
            start_customisation(&mut first, &host).installed,
            CustomisationOrigin::BuiltIn
        );
        let based_on = first
            .customisation_state()
            .based_on
            .as_ref()
            .expect("based on itself");
        assert_eq!(based_on.name, "Small Built In");
        assert_eq!(based_on.digest, fnv1a(&exported));

        // Kept, by hand as KEEP will, and started again.
        let text = customisation_to_json(
            &first.customisation(),
            &CustomisationWriteOptions::default(),
        )
        .unwrap_or_else(|error| panic!("{}", error.describe()));
        scratch.write("customisation.json", &text);
        let mut second = Document::default();
        let report = start_customisation(&mut second, &host);
        assert_eq!(report.installed, CustomisationOrigin::Kept);
        assert!(report.problems.is_empty());
        assert!(!report.kept_from_another_built_in);
    }

    #[test]
    fn starting_over_a_changed_session_gives_the_session_a_fresh_start_gives() {
        // What a reset to the built-in is: the built-in installed over a session
        // that was changed since. The small built-in spells start "S" and says
        // nothing of the automation. By hand, the reset session spells start "S"
        // again and has both switches ON, the defaults - the very session a new
        // Document started with the same host has, which is what `kept` means.
        let host = host_of(built_in_from(SMALL));
        let mut changed = Document::default();
        assert_eq!(
            start_customisation(&mut changed, &host).installed,
            CustomisationOrigin::BuiltIn
        );
        let spelled = LineworkCodes {
            start: String::from("BEG"),
            ..LineworkCodes::default()
        };
        assert!(changed.set_linework_codes(spelled).is_ok());
        changed.set_automation(CustomisationAutomation {
            codes_on_survey_import: false,
            linework_on_survey_import: false,
        });
        assert!(!changed.customisation_state().kept);

        assert_eq!(
            start_customisation(&mut changed, &host).installed,
            CustomisationOrigin::BuiltIn
        );
        let mut fresh = Document::default();
        assert_eq!(
            start_customisation(&mut fresh, &host).installed,
            CustomisationOrigin::BuiltIn
        );

        assert_eq!(changed.customisation_state(), fresh.customisation_state());
        assert_eq!(changed.customisation_state().linework.start, "S");
        assert!(
            changed
                .customisation_state()
                .automation
                .codes_on_survey_import
        );
        assert!(
            changed
                .customisation_state()
                .automation
                .linework_on_survey_import
        );
        assert!(changed.customisation_state().kept);
    }

    #[test]
    fn a_kept_file_that_does_not_read_is_reported_and_the_built_in_stands() {
        let scratch = Scratch::new("start-unreadable");
        let host = CustomisationHost {
            built_in: built_in_from(SMALL),
            kept_file: scratch.write("customisation.json", "{ this is not a customisation"),
            ..CustomisationHost::default()
        };

        let mut document = Document::default();
        let report = start_customisation(&mut document, &host);

        assert_eq!(report.installed, CustomisationOrigin::BuiltIn);
        assert_eq!(report.name, "Small Built In");
        assert_eq!(report.problems.len(), 1);
        // Which file, why, and what was done instead.
        assert!(
            report.problems[0].contains("customisation.json"),
            "{}",
            report.problems[0]
        );
        assert!(
            report.problems[0].contains("not a Katana customisation file"),
            "{}",
            report.problems[0]
        );
        assert!(
            report.problems[0].contains("the built-in customisation is used"),
            "{}",
            report.problems[0]
        );
        assert_eq!(
            document.customisation_state().origin,
            CustomisationOrigin::BuiltIn
        );
        assert!(document.customisation_state().kept);
        assert_eq!(document.style_library().size(), 2);

        // With no built-in to stand in, nothing is loaded, and that is said.
        let bare = CustomisationHost {
            kept_file: host.kept_file.clone(),
            ..CustomisationHost::default()
        };
        let mut empty = Document::default();
        let nothing = start_customisation(&mut empty, &bare);
        assert_eq!(nothing.installed, CustomisationOrigin::None);
        assert_eq!(nothing.problems.len(), 1);
        assert!(
            nothing.problems[0].contains("no customisation is loaded"),
            "{}",
            nothing.problems[0]
        );
        assert_eq!(
            empty.customisation_state().origin,
            CustomisationOrigin::None
        );
        assert!(empty.style_library().empty());
    }

    #[test]
    fn a_built_in_that_was_not_read_is_reported_and_nothing_is_installed() {
        // What built_in_customisation gives for a file the seam names that does
        // not read: none, and why.
        let host = host_of(BuiltInCustomisation {
            problem: String::from("the built-in customisation is not read: it is damaged"),
            ..BuiltInCustomisation::default()
        });
        let mut document = Document::default();
        let report = start_customisation(&mut document, &host);
        assert_eq!(report.installed, CustomisationOrigin::None);
        assert_eq!(report.problems, vec![host.built_in.problem.clone()]);
        assert_eq!(
            document.customisation_state().origin,
            CustomisationOrigin::None
        );
        assert!(document.customisation_state().built_in.is_empty());
    }

    #[test]
    fn a_built_in_the_document_refuses_is_reported_and_nothing_is_installed() {
        // Built in code, as no file could be read: a name no project could
        // record.
        let unusable = Customisation {
            name: String::from("roads/2026"),
            ..Customisation::default()
        };
        let host = host_of(BuiltInCustomisation {
            customisation: Some(Arc::new(unusable)),
            ..BuiltInCustomisation::default()
        });
        let mut document = Document::default();
        let report = start_customisation(&mut document, &host);
        assert_eq!(report.installed, CustomisationOrigin::None);
        assert_eq!(report.problems.len(), 1);
        assert!(
            report.problems[0].starts_with("the built-in customisation is not installed: "),
            "{}",
            report.problems[0]
        );
        assert_eq!(
            document.customisation_state().origin,
            CustomisationOrigin::None
        );
        assert!(!document.customisation_state().kept);
    }

    #[test]
    fn what_a_start_found_stays_on_the_document_and_is_in_its_json_report() {
        // A front end says what a start found once, where its errors go. Whoever
        // asks what the session holds later - a client of katana_mcp, which never
        // sees that stream - was told `origin: builtIn` and not that the
        // customisation it kept had been refused. So it is kept on the Document,
        // and the report gives it.
        let scratch = Scratch::new("start-on-the-document");
        let mut host = CustomisationHost {
            built_in: built_in_from(SMALL),
            kept_file: scratch.write("customisation.json", "{ this is not a customisation"),
            ..CustomisationHost::default()
        };

        let mut document = Document::default();
        let report = start_customisation(&mut document, &host);
        let sentence = "the kept customisation is not read, so the built-in customisation is \
                        used: ParseFailure: not a Katana customisation file";
        assert_eq!(report.problems.len(), 1);
        assert!(
            report.problems[0].starts_with(sentence),
            "{}",
            report.problems[0]
        );
        assert_eq!(
            document.customisation_state().start_problems,
            report.problems
        );
        assert!(!document.customisation_state().kept_from_another_built_in);
        // Keys in alphabetical order, two blanks a level: `start` is the last
        // member, and the sentence begins its one line.
        let json = customisation_json(&document);
        assert!(
            json.contains(&format!(
                "\"start\": {{\n    \"keptFromAnotherBuiltIn\": false,\n    \"problems\": [\n      \
                 \"{sentence}"
            )),
            "{json}"
        );

        // It is of the START: an edit of the session and a load into it change
        // neither.
        document.set_automation(CustomisationAutomation {
            codes_on_survey_import: false,
            linework_on_survey_import: false,
        });
        let loaded = customisation_from_json(kept("").as_bytes())
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(
            document
                .install_customisation(loaded, CustomisationOrigin::Loaded, false)
                .is_ok()
        );
        assert_eq!(document.customisation_state().name, "Mine");
        assert_eq!(
            document.customisation_state().start_problems,
            report.problems
        );

        // A kept customisation made from another edition of the built-in is no
        // problem, and is said too.
        host.kept_file = scratch.write("earlier.json", &kept("0123456789abcdef"));
        let mut earlier = Document::default();
        assert!(start_customisation(&mut earlier, &host).kept_from_another_built_in);
        assert!(earlier.customisation_state().kept_from_another_built_in);
        assert!(earlier.customisation_state().start_problems.is_empty());
        assert!(customisation_json(&earlier).contains(
            "\"start\": {\n    \"keptFromAnotherBuiltIn\": true,\n    \"problems\": []\n  }"
        ));

        // A start that found nothing leaves nothing - over what an earlier start
        // left, too: the first Document, started again with no kept file.
        let nothing =
            "\"start\": {\n    \"keptFromAnotherBuiltIn\": false,\n    \"problems\": []\n  }";
        let clean = host_of(host.built_in.clone());
        assert!(
            start_customisation(&mut document, &clean)
                .problems
                .is_empty()
        );
        assert!(document.customisation_state().start_problems.is_empty());
        assert!(customisation_json(&document).contains(nothing));
        // And a Document that was never started says the same.
        let never = Document::default();
        assert!(customisation_json(&never).contains(nothing));
    }

    #[test]
    fn recording_what_a_start_found_is_no_edit_of_the_session_and_is_reported_once() {
        let mut document = Document::default();
        let small = customisation_from_json(SMALL.as_bytes())
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(
            document
                .install_customisation(small, CustomisationOrigin::BuiltIn, true)
                .is_ok()
        );
        let calls: Arc<Mutex<Vec<u32>>> = Arc::default();
        let sink = Arc::clone(&calls);
        let _handle = document.add_change_listener(move |change: &DocumentChange<'_>| {
            sink.lock().expect("the list of calls").push(change.parts);
        });
        let before = document.customisation_generation();
        let problems = vec![String::from("the kept customisation is not read")];

        document.set_customisation_start(problems.clone(), true);
        // One notification, of the customisation's state alone, as for every
        // other part of it.
        {
            let seen = calls.lock().expect("the list of calls");
            assert_eq!(seen.len(), 1);
            assert_eq!(seen[0], DocumentChange::CUSTOMISATION);
        }
        assert_eq!(document.customisation_generation(), before + 1);
        let state = document.customisation_state();
        assert_eq!(state.start_problems, problems);
        assert!(state.kept_from_another_built_in);
        // What a start found is no edit: the session is still the built-in, still
        // what the next start would give, and the drawing is untouched.
        assert_eq!(state.origin, CustomisationOrigin::BuiltIn);
        assert!(state.kept);
        assert!(!document.is_modified());

        // The value it already has is nothing at all.
        document.set_customisation_start(problems, true);
        assert_eq!(calls.lock().expect("the list of calls").len(), 1);
        assert_eq!(document.customisation_generation(), before + 1);
    }

    #[test]
    fn a_session_written_to_the_kept_file_is_the_session_the_next_start_gives() {
        // What KEEP will do, by hand: the session as one customisation, written
        // as a file's text to the kept file. The next start, of another Document
        // with the same host, reads it back as the same session - kept, from the
        // same built-in.
        let scratch = Scratch::new("start-round-trip");
        let host = CustomisationHost {
            built_in: built_in_from(SMALL),
            kept_file: scratch.root.join("customisation.json"),
            ..CustomisationHost::default()
        };

        let mut first = Document::default();
        assert_eq!(
            start_customisation(&mut first, &host).installed,
            CustomisationOrigin::BuiltIn
        );
        // An edit, so that what is kept is not simply the built-in.
        first.set_automation(CustomisationAutomation {
            codes_on_survey_import: false,
            linework_on_survey_import: true,
        });
        let session = first.customisation();
        let text = customisation_to_json(&session, &CustomisationWriteOptions::default())
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        scratch.write("customisation.json", &text);

        let mut second = Document::default();
        let report = start_customisation(&mut second, &host);
        assert_eq!(report.installed, CustomisationOrigin::Kept);
        assert!(report.problems.is_empty());
        assert!(!report.kept_from_another_built_in);
        assert!(second.customisation() == session);
        assert!(
            !second
                .customisation_state()
                .automation
                .codes_on_survey_import
        );
        assert!(second.customisation_state().kept);
        assert_eq!(
            second.customisation_state().origin,
            CustomisationOrigin::Kept
        );
    }

    // ---- the built-ins a program offers, and a kept copy of one of them ----------------------
    //
    // A kept customisation says what it is based on, by name and digest. It is
    // "from another built-in" when that is neither the run's own nor any other the
    // program offers in the edition it has - so choosing an alternate and keeping
    // it (CUSTOMISE BUILTIN, then KEEP) survives a restart without a warning, and
    // survives a rebuild that makes another built-in the default.

    #[test]
    fn a_kept_copy_of_an_alternate_the_program_offers_is_not_from_another_built_in() {
        let scratch = Scratch::new("start-kept-alternate");
        let host = CustomisationHost {
            built_in: built_in_from(SMALL),
            built_ins: Some(vec![entry_of(SMALL), entry_of(ALTERNATE)]),
            kept_file: scratch.write(
                "customisation.json",
                &kept_text(&fnv1a(ALTERNATE), "Alternate One"),
            ),
        };

        let mut document = Document::default();
        let report = start_customisation(&mut document, &host);
        assert_eq!(report.installed, CustomisationOrigin::Kept);
        assert_eq!(report.name, "Mine");
        assert!(
            !report.kept_from_another_built_in,
            "made from a built-in this program has"
        );
        assert!(report.problems.is_empty());
        // The run's built-in is still the default, whatever the kept copy is from.
        assert_eq!(document.customisation_state().built_in, "Small Built In");
        assert!(!document.customisation_state().kept_from_another_built_in);
    }

    #[test]
    fn a_kept_copy_of_an_alternate_is_from_another_built_in_when_the_program_lacks_it_or_has_another_edition()
     {
        let scratch = Scratch::new("start-kept-alternate-moved");
        let mut host = CustomisationHost {
            built_in: built_in_from(SMALL),
            built_ins: Some(vec![entry_of(SMALL), entry_of(ALTERNATE)]),
            ..CustomisationHost::default()
        };

        // Another edition of the alternate: its bytes are not the ones it was
        // kept from.
        host.kept_file = scratch.write(
            "edition.json",
            &kept_text("0123456789abcdef", "Alternate One"),
        );
        let mut edition = Document::default();
        assert!(start_customisation(&mut edition, &host).kept_from_another_built_in);
        assert!(edition.customisation_state().kept_from_another_built_in);

        // A built-in this program does not offer (a rebuild dropped it).
        host.kept_file = scratch.write("gone.json", &kept_text(&fnv1a(ALTERNATE), "Dropped One"));
        let mut gone = Document::default();
        assert!(start_customisation(&mut gone, &host).kept_from_another_built_in);

        // Offered by no list at all: a host with an empty one.
        host.built_ins = Some(Vec::new());
        host.kept_file = scratch.write(
            "none-offered.json",
            &kept_text(&fnv1a(ALTERNATE), "Alternate One"),
        );
        let mut none = Document::default();
        assert!(
            start_customisation(&mut none, &host).kept_from_another_built_in,
            "the run's own is Small; an alternate that is not offered is another built-in"
        );
    }

    #[test]
    fn a_kept_copy_of_the_earlier_default_survives_a_rebuild_that_changes_the_default() {
        // Kept when "Small Built In" was the default; the program now starts with
        // "Alternate One" and still offers the other. Nothing has moved on: the
        // copy is of a built-in this program has, in the edition it has.
        let scratch = Scratch::new("start-kept-earlier-default");
        let mut host = CustomisationHost {
            built_in: built_in_from(ALTERNATE),
            built_ins: Some(vec![entry_of(ALTERNATE), entry_of(SMALL)]),
            kept_file: scratch.write("customisation.json", &kept(&fnv1a(SMALL))),
        };

        let mut document = Document::default();
        let report = start_customisation(&mut document, &host);
        assert_eq!(report.installed, CustomisationOrigin::Kept);
        assert!(!report.kept_from_another_built_in);
        assert_eq!(document.customisation_state().built_in, "Alternate One");

        // And if the program no longer offers the earlier default, it says so.
        host.built_ins = Some(vec![entry_of(ALTERNATE)]);
        let mut dropped = Document::default();
        assert!(start_customisation(&mut dropped, &host).kept_from_another_built_in);
    }

    #[test]
    fn with_no_built_in_of_its_own_an_alternate_offered_is_still_not_started_with() {
        // A run whose default is none (the seam said `none`, or the build has no
        // default): the host may still offer built-ins by name, and a start with
        // no kept file installs nothing and says nothing.
        let scratch = Scratch::new("start-alternates-only");
        let mut host = CustomisationHost {
            built_ins: Some(vec![entry_of(SMALL), entry_of(ALTERNATE)]),
            kept_file: scratch.root.join("customisation.json"),
            ..CustomisationHost::default()
        };

        let mut document = Document::default();
        let report = start_customisation(&mut document, &host);
        assert_eq!(report.installed, CustomisationOrigin::None);
        assert!(report.problems.is_empty());
        assert!(document.customisation_state().built_in.is_empty());

        // A kept copy of one of them says nothing of another built-in, as with no
        // list.
        host.kept_file = scratch.write("kept.json", &kept(&fnv1a(SMALL)));
        let mut kept_document = Document::default();
        let again = start_customisation(&mut kept_document, &host);
        assert_eq!(again.installed, CustomisationOrigin::Kept);
        assert!(!again.kept_from_another_built_in);
    }
}
