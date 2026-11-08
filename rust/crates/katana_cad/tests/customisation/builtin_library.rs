//! The built-in LIBRARY (customisation_host): the several customisations a program is built with,
//! the default first, found by name; the seam's name form; and how a session stands to them
//! (built_in_choices) (tests/cad/customisation/test_builtin_library.cpp).
//!
//! Two halves, and the first needs no build to have anything: the rules are tested against
//! stand-ins the test writes out (the shared support's ALPHA and BETA), so they are the same tests
//! on a machine whose build compiled in other files. The second asks the BUILD what it compiled in
//! and holds each file to the figures of its own census; it SKIPS what this build did not compile
//! in, so the suite stays green without it. The cases that set the seam in the environment, or
//! change the working folder, are in the target tests/customisation_env.
//!
//! Katana Standard's figures are those of its census (tools/katana_standard/census.py, held in
//! test_katana_standard.cpp): 221 definitions (98 linestyles, 123 symbols), 1103 rules over 632
//! keys, 28 colours. NSW's are test_builtin_census.cpp's: 792, 1,624 rules over 632 keys.

use katana_cad::Document;
use katana_cad::customisation_host::{
    BuiltInChoice, BuiltInCustomisation, BuiltInEntry, BuiltInFile, CustomisationHost,
    built_in_advice_for, built_in_choices, built_in_customisation_for, built_ins, built_ins_from,
    built_ins_of, compiled_in_customisation, find_built_in, install_built_in_customisation,
    start_customisation,
};
use katana_cad::customisation_state::CustomisationOrigin;
use katana_core::path_text::path_to_utf8;
use katana_entity::CustomisationAutomation;
use katana_entity::customisation::customisation_digest;

use crate::customisation_support::{ALPHA, BETA, Scratch, entry_from, same, two_entries};

fn files<'a>(list: &[(&'a str, &'a str)]) -> Vec<BuiltInFile<'a>> {
    list.iter()
        .map(|&(file, text)| BuiltInFile {
            file,
            bytes: text.as_bytes(),
        })
        .collect()
}

fn choice_named<'a>(choices: &'a [BuiltInChoice], name: &str) -> Option<&'a BuiltInChoice> {
    choices.iter().find(|choice| choice.name == name)
}

fn damaged(problem: &str) -> BuiltInEntry {
    BuiltInEntry {
        name: String::from("damaged.customisation.json"),
        source: String::from("damaged.customisation.json"),
        built_in: BuiltInCustomisation {
            problem: String::from(problem),
            ..BuiltInCustomisation::default()
        },
        ..BuiltInEntry::default()
    }
}

fn names(missing: &[&str]) -> Vec<String> {
    missing.iter().map(|name| String::from(*name)).collect()
}

mod built_in_library {
    use super::*;

    // ---- finding one by name ------------------------------------------------------------

    #[test]
    fn an_entry_is_found_by_its_name_in_any_letter_case_and_only_by_its_whole_name() {
        let entries = two_entries();
        for spelling in ["Beta Two", "beta two", "BETA TWO", "bEtA tWo"] {
            let found = find_built_in(&entries, spelling).expect(spelling);
            assert_eq!(found.name, "Beta Two", "{spelling}");
        }
        assert_eq!(
            find_built_in(&entries, "Alpha").map(|entry| entry.name.as_str()),
            Some("Alpha")
        );
        // The whole name: not a part of it, not with a blank more, and not empty.
        for wrong in [
            "Beta",
            "Two",
            "Beta Two ",
            " Beta Two",
            "Alph",
            "",
            "alpha.customisation.json",
        ] {
            assert!(find_built_in(&entries, wrong).is_none(), "\"{wrong}\"");
        }
        assert!(find_built_in(&[], "Alpha").is_none());
    }

    // ---- the list built from files ------------------------------------------------------
    //
    // built_ins() is built_ins_from over what the build compiled in, so its rules
    // are tested here over files the test writes out: no build can reach a second
    // file of one name, or a file that is no customisation, unless it is made so.

    #[test]
    fn a_list_from_files_holds_them_in_their_order_with_their_names_sources_and_digests() {
        let given = files(&[
            ("alpha.customisation.json", ALPHA),
            ("beta.customisation.json", BETA),
        ]);
        let list = built_ins_from(&given, None);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name, "Alpha");
        assert_eq!(list[0].description, "The first stand-in");
        assert_eq!(list[0].source, "alpha.customisation.json");
        assert_eq!(
            list[0].built_in.digest,
            customisation_digest(ALPHA.as_bytes())
        );
        assert!(list[0].built_in.customisation.is_some());
        assert_eq!(list[1].name, "Beta Two");
        assert_eq!(list[1].source, "beta.customisation.json");
        assert_eq!(
            list[1].built_in.digest,
            customisation_digest(BETA.as_bytes())
        );
        assert!(
            built_ins_from(&[], None).is_empty(),
            "no files, no built-ins"
        );
    }

    #[test]
    fn the_first_file_takes_the_parse_it_is_given_and_only_the_first() {
        let parsed = two_entries();
        let given = files(&[
            ("alpha.customisation.json", ALPHA),
            ("beta.customisation.json", BETA),
        ]);
        // The default is the parse compiled_in_customisation() made: shared, not
        // read twice.
        let shared = built_ins_from(&given, Some(&parsed[0].built_in));
        assert_eq!(shared.len(), 2);
        assert!(same(
            &shared[0].built_in.customisation,
            &parsed[0].built_in.customisation
        ));
        assert!(
            !same(
                &shared[1].built_in.customisation,
                &parsed[1].built_in.customisation
            ),
            "the second file is read here, whatever the first was given"
        );
        // Handed none, every file is read, so the first is a parse of its own.
        let read = built_ins_from(&given, None);
        assert!(!same(
            &read[0].built_in.customisation,
            &parsed[0].built_in.customisation
        ));
        assert_eq!(read[0].name, "Alpha");
    }

    #[test]
    fn a_later_file_with_an_earlier_ones_name_in_any_letter_case_is_not_offered() {
        // The copy says it is "ALPHA", and has the second stand-in's single
        // definition.
        let copy = BETA.replacen("\"name\": \"Beta Two\"", "\"name\": \"ALPHA\"", 1);
        let given = files(&[
            ("alpha.customisation.json", ALPHA),
            ("copy.customisation.json", copy.as_str()),
            ("beta.customisation.json", BETA),
        ]);
        let list = built_ins_from(&given, None);
        assert_eq!(list.len(), 2, "a name picks one entry");
        assert_eq!(
            list[0].source, "alpha.customisation.json",
            "the earlier file keeps the name"
        );
        let first = list[0].built_in.customisation.as_ref().expect("Alpha read");
        assert!(
            first.library.find("ALPHA Line").is_some(),
            "the earlier file's own contents"
        );
        assert!(first.library.find("BETA Line").is_none());
        assert_eq!(
            list[1].source, "beta.customisation.json",
            "the files after it are still read"
        );
        assert_eq!(
            find_built_in(&list, "alpha").map(|entry| entry.source.as_str()),
            Some("alpha.customisation.json")
        );
    }

    #[test]
    fn a_file_that_is_no_customisation_is_listed_by_its_file_name_with_the_reason() {
        let given = files(&[
            ("broken.customisation.json", "{ not a customisation"),
            ("beta.customisation.json", BETA),
        ]);
        let list = built_ins_from(&given, None);
        assert_eq!(list.len(), 2);
        // Listed, so that a person can see that it is there and why it is of no
        // use...
        assert_eq!(list[0].name, "broken.customisation.json");
        assert_eq!(list[0].source, "broken.customisation.json");
        assert!(list[0].built_in.customisation.is_none());
        assert!(
            list[0]
                .built_in
                .problem
                .contains("broken.customisation.json"),
            "{}",
            list[0].built_in.problem
        );
        assert!(
            list[0].built_in.problem.contains("is not read"),
            "{}",
            list[0].built_in.problem
        );
        // ...and the next file is not held up by it.
        assert_eq!(list[1].name, "Beta Two");
        assert!(list[1].built_in.customisation.is_some());

        // A default that did not read was parsed by someone else, and is taken
        // as it is: its reason is theirs, its place the file name.
        let failed = BuiltInCustomisation {
            problem: String::from("the reason the default was refused"),
            ..BuiltInCustomisation::default()
        };
        let with_failed = built_ins_from(&given, Some(&failed));
        assert_eq!(with_failed.len(), 2);
        assert_eq!(with_failed[0].name, "broken.customisation.json");
        assert_eq!(
            with_failed[0].built_in.problem,
            "the reason the default was refused"
        );
    }

    // ---- what to tell of a name that is a built-in ----------------------------------------

    #[test]
    fn a_missing_name_that_is_a_built_in_is_told_with_the_verb_that_makes_it_the_session() {
        let entries = two_entries();
        let advice = |missing: &[&str], offered: &[BuiltInEntry]| {
            built_in_advice_for(&names(missing), offered)
        };
        // Spelled as the entry spells it, whatever case the project recorded.
        assert_eq!(
            advice(&["beta two"], &entries),
            "CUSTOMISE BUILTIN Beta Two makes the built-in \"Beta Two\" the session"
        );
        // A name that is no built-in's, among others that are not, says
        // nothing...
        assert_eq!(advice(&["Site", "streets.4d"], &entries), "");
        // ...and one that is, among those, is said alone.
        assert_eq!(
            advice(&["Site", "ALPHA"], &entries),
            "CUSTOMISE BUILTIN Alpha makes the built-in \"Alpha\" the session"
        );
        // Two of them: a clause each, in the project's order.
        assert_eq!(
            advice(&["Beta Two", "Alpha"], &entries),
            "CUSTOMISE BUILTIN Beta Two makes the built-in \"Beta Two\" the session; CUSTOMISE \
             BUILTIN Alpha makes the built-in \"Alpha\" the session"
        );
        assert_eq!(advice(&[], &entries), "");
        assert_eq!(advice(&["Alpha"], &[]), "", "a program with no built-in");
    }

    #[test]
    fn a_built_in_that_did_not_read_is_not_offered_to_a_project_that_names_it() {
        let mut entries = two_entries();
        entries.push(damaged("not read"));
        // CUSTOMISE BUILTIN would refuse it, so the words do not send anyone to
        // it.
        assert_eq!(
            built_in_advice_for(&names(&["damaged.customisation.json"]), &entries),
            ""
        );
    }

    // ---- the seam: the name form ----------------------------------------------------------

    #[test]
    fn the_seam_names_a_compiled_in_built_in_and_that_built_in_is_the_runs_own() {
        let entries = two_entries();
        let stand_in = entries[0].built_in.clone();
        for name in ["Beta Two", "beta two", "BETA TWO"] {
            let built_in = built_in_customisation_for(name, &stand_in, &entries);
            let customisation = built_in
                .customisation
                .as_ref()
                .unwrap_or_else(|| panic!("{name}: {}", built_in.problem));
            assert_eq!(customisation.name, "Beta Two", "{name}");
            // The very one the list holds, shared, with its digest.
            assert!(
                same(&built_in.customisation, &entries[1].built_in.customisation),
                "{name}"
            );
            assert_eq!(built_in.digest, entries[1].built_in.digest, "{name}");
            assert!(built_in.problem.is_empty(), "{name}");
        }
        // Unset still gives the default the program has, and `none` still none:
        // the name form takes nothing from either.
        assert!(same(
            &built_in_customisation_for("", &stand_in, &entries).customisation,
            &stand_in.customisation
        ));
        assert!(
            built_in_customisation_for("none", &stand_in, &entries)
                .customisation
                .is_none()
        );
    }

    #[test]
    fn none_beats_a_built_in_of_that_name_and_a_path_beats_a_name() {
        // A built-in called "none" is not reachable by the seam: `none` is no
        // built-in.
        let mut entries = two_entries();
        entries.push(entry_from(
            r#"{"format": "katana-customisation", "version": 1, "name": "None",
 "codes": [{"key": "NN*", "sets": "feature", "layer": "NONE"}]}"#,
            "",
        ));
        assert!(entries[2].built_in.customisation.is_some());
        let none = built_in_customisation_for("None", &BuiltInCustomisation::default(), &entries);
        assert!(none.customisation.is_none());
        assert!(none.problem.is_empty(), "asked for, so not a problem");

        // A value with a folder in it is a path, even when what follows the last
        // separator is a name: the file is read, and the built-in of that name is
        // not.
        let scratch = Scratch::new("library-path-beats-name");
        let file = scratch.write("Alpha", BETA);
        let read = built_in_customisation_for(path_to_utf8(&file), &entries[0].built_in, &entries);
        let customisation = read.customisation.as_ref().expect(&read.problem);
        assert_eq!(
            customisation.name, "Beta Two",
            "the file's, not the list's Alpha"
        );
    }

    #[test]
    fn a_built_in_that_did_not_read_is_handed_on_with_its_reason_when_its_name_is_asked() {
        let entries = [damaged(
            "the customisation compiled into this program is not read: damaged",
        )];
        let built_in = built_in_customisation_for(
            "damaged.customisation.json",
            &BuiltInCustomisation::default(),
            &entries,
        );
        assert!(built_in.customisation.is_none());
        assert_eq!(built_in.problem, entries[0].built_in.problem);
    }

    // ---- the choices ----------------------------------------------------------------------

    #[test]
    fn each_choice_counts_what_its_built_in_holds_and_knows_the_default_and_the_active_one() {
        let entries = two_entries();
        let host = CustomisationHost {
            built_in: entries[0].built_in.clone(),
            built_ins: Some(entries.clone()),
            ..CustomisationHost::default()
        };
        let mut document = Document::default();
        assert_eq!(
            start_customisation(&mut document, &host).installed,
            CustomisationOrigin::BuiltIn
        );

        let choices = built_in_choices(&document, &entries);
        assert_eq!(choices.len(), 2);
        assert_eq!(choices[0].name, "Alpha");
        assert_eq!(choices[0].description, "The first stand-in");
        assert_eq!(choices[0].source, "alpha.customisation.json");
        assert_eq!(choices[0].definitions, 2);
        assert_eq!(choices[0].rules, 3);
        assert_eq!(choices[0].codes, 2);
        assert!(choices[0].is_default);
        assert!(choices[0].is_active);
        assert!(choices[0].problem.is_empty());
        assert_eq!(choices[1].name, "Beta Two");
        assert_eq!(choices[1].definitions, 1);
        assert_eq!(choices[1].rules, 1);
        assert_eq!(choices[1].codes, 1);
        assert!(!choices[1].is_default);
        assert!(!choices[1].is_active);

        // Another built-in in the session's place: it is the active one, the
        // default stays the run's.
        assert!(install_built_in_customisation(&mut document, &entries[1].built_in, false).is_ok());
        let after = built_in_choices(&document, &entries);
        assert!(after[0].is_default);
        assert!(!after[0].is_active);
        assert!(!after[1].is_default);
        assert!(after[1].is_active);
    }

    #[test]
    fn a_copy_of_a_built_in_is_not_the_active_one_and_a_session_with_no_host_has_no_default() {
        let entries = two_entries();
        // The built-in, installed and then edited: it is a copy, not the built-in.
        let mut edited = Document::default();
        assert!(install_built_in_customisation(&mut edited, &entries[0].built_in, true).is_ok());
        edited.set_automation(CustomisationAutomation {
            codes_on_survey_import: false,
            linework_on_survey_import: false,
        });
        for choice in built_in_choices(&edited, &entries) {
            assert!(!choice.is_active, "{}", choice.name);
            assert!(
                !choice.is_default,
                "no host told the Document what the run's built-in is"
            );
        }
        // A Document with nothing in it, and an empty list.
        let empty = Document::default();
        assert!(built_in_choices(&empty, &[]).is_empty());
        for choice in built_in_choices(&empty, &entries) {
            assert!(!choice.is_active);
        }
    }

    #[test]
    fn a_choice_that_did_not_read_has_its_reason_and_no_counts() {
        let broken = BuiltInEntry {
            source: String::new(),
            ..damaged("not read")
        };
        let document = Document::default();
        let choices = built_in_choices(&document, std::slice::from_ref(&broken));
        assert_eq!(choices.len(), 1);
        assert_eq!(choices[0].problem, "not read");
        assert_eq!(choices[0].definitions, 0);
        assert_eq!(choices[0].rules, 0);
        assert!(!choices[0].is_active);
    }

    #[test]
    fn a_host_offers_its_own_list_and_otherwise_what_was_compiled_in() {
        let mut host = CustomisationHost::default();
        assert!(std::ptr::eq(built_ins_of(&host), built_ins()));
        host.built_ins = Some(two_entries());
        let own = host.built_ins.as_deref().expect("a list was given");
        assert!(std::ptr::eq(built_ins_of(&host), own));
        host.built_ins = Some(Vec::new());
        assert!(built_ins_of(&host).is_empty(), "an empty list is a list");
    }

    // ---- what this build compiled in ------------------------------------------------------

    #[test]
    fn the_compiled_in_list_has_the_default_first_shared_and_every_name_once() {
        let entries = built_ins();
        assert!(std::ptr::eq(entries, built_ins()), "parsed once");
        let compiled_in = compiled_in_customisation();
        if entries.is_empty() {
            assert!(compiled_in.customisation.is_none());
            eprintln!("this build compiled no built-in customisation in");
            return;
        }
        for (at, entry) in entries.iter().enumerate() {
            assert!(!entry.name.is_empty(), "{at}");
            assert!(!entry.source.is_empty(), "{at}: compiled in from a file");
            assert!(
                entry.built_in.problem.is_empty(),
                "{}: {}",
                entry.name,
                entry.built_in.problem
            );
            // One name, one entry, whatever the letter case.
            assert!(
                find_built_in(entries, &entry.name).is_some_and(|found| std::ptr::eq(found, entry))
            );
        }
        if compiled_in.customisation.is_some() {
            // The default is the first, and the one parse is shared.
            assert!(same(
                &entries[0].built_in.customisation,
                &compiled_in.customisation
            ));
            assert_eq!(entries[0].built_in.digest, compiled_in.digest);
        }
    }

    #[test]
    fn the_default_build_compiles_katana_standard_in_as_the_default_and_it_holds_its_census() {
        let entries = built_ins();
        if entries
            .first()
            .is_none_or(|first| first.source != "katana-standard.customisation.json")
        {
            eprintln!("this build's default built-in is not Katana Standard");
            return;
        }
        let compiled_in = compiled_in_customisation();
        let customisation = compiled_in
            .customisation
            .as_ref()
            .expect(&compiled_in.problem);
        assert_eq!(customisation.name, "Katana Standard");
        // The census figures, as test_katana_standard holds the file to them:
        // here it is the COMPILED-IN copy, which is what a program starts with.
        assert_eq!(customisation.library.size(), 221);
        assert_eq!(customisation.map.size(), 1103);
        assert_eq!(customisation.map.keys().len(), 632);
        assert_eq!(customisation.colours.size(), 28);
        assert_eq!(compiled_in.digest.len(), 16);
        // And its choice figures say the same.
        let document = Document::default();
        let choices = built_in_choices(&document, entries);
        let standard = choice_named(&choices, "Katana Standard").expect("Katana Standard");
        assert_eq!(standard.definitions, 221);
        assert_eq!(standard.rules, 1103);
        assert_eq!(standard.codes, 632);
        assert_eq!(standard.source, "katana-standard.customisation.json");
    }

    #[test]
    fn the_nsw_alternate_is_listed_after_the_default_and_never_is_it() {
        let Some(nsw) = find_built_in(built_ins(), "NSW") else {
            eprintln!("this build has no NSW customisation compiled in");
            return;
        };
        assert!(
            nsw.built_in.customisation.is_some(),
            "{}",
            nsw.built_in.problem
        );
        assert_eq!(nsw.source, "nsw.customisation.json");
        // test_builtin_census's figures.
        let document = Document::default();
        let choices = built_in_choices(&document, std::slice::from_ref(nsw));
        let choice = choice_named(&choices, "NSW").expect("its choice");
        assert_eq!(choice.definitions, 792);
        assert_eq!(choice.rules, 1624);
        assert_eq!(choice.codes, 632);
        // It is an alternate: the default, when the build has one, is another.
        if let Some(default) = &compiled_in_customisation().customisation {
            assert_ne!(
                default.name, "NSW",
                "the default build's default is Katana Standard"
            );
            assert!(!same(
                &compiled_in_customisation().customisation,
                &nsw.built_in.customisation
            ));
            assert!(!std::ptr::eq(&built_ins()[0], nsw));
        }
    }
}
