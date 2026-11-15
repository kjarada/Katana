// What a project records of the customisation it was drawn with, and what
// opening it says is missing (storage's ProjectMetadata::customisation;
// tests/cad/customisation/test_customisation_record.cpp). Hand-built
// libraries: the names are what matter, not what the files draw.

use std::collections::BTreeSet;
use std::path::PathBuf;

use katana_cad::customisation_record::{
    builtin_definition_renames, builtin_renames, customisation_not_loaded, customisation_record,
    customisation_record_to_save, definition_name_now, definition_name_now_in,
    distinct_customisation_files, note_customisation_loaded, record_customisation_load,
    source_name_hash, typed_save_has_destination,
};
use katana_cad::{CustomisationSource, RenamedDefinition, RenamedSource};
use katana_entity::{LineStyle, Stroke, StrokeOp, StyleLibrary};
use katana_geometry::Point2;

fn definition(name: &str, source: &str) -> LineStyle {
    LineStyle {
        name: String::from(name),
        source: String::from(source),
        strokes: vec![Stroke {
            op: StrokeOp::Draw,
            point: Point2::new(1.0, 0.0),
            ..Stroke::default()
        }],
        ..LineStyle::default()
    }
}

fn source(name: &str, definitions: bool, rules: bool) -> CustomisationSource {
    CustomisationSource {
        name: String::from(name),
        definitions,
        rules,
        notice: Vec::new(),
    }
}

// A source is {name, brought definitions, brought rules, notice}: a style
// library brought definitions and a survey code file rules. (One `library`
// bool said which before one customisation could bring both.)
fn library(name: &str) -> CustomisationSource {
    source(name, true, false)
}

fn mapfile(name: &str) -> CustomisationSource {
    source(name, false, true)
}

// One Katana customisation, which brings both.
fn whole(name: &str) -> CustomisationSource {
    source(name, true, true)
}

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|name| String::from(*name)).collect()
}

fn add(styles: &mut StyleLibrary, name: &str, source: &str) {
    assert!(styles.add(definition(name, source)).is_ok());
}

fn renamed(former: &str, now: &str) -> RenamedSource {
    RenamedSource {
        former_name: source_name_hash(former),
        now: String::from(now),
        ..RenamedSource::default()
    }
}

// No rename is in play. customisation_not_loaded, note_customisation_loaded
// and customisation_record_to_save each had a form that took no table and
// used the built-in's, and five tests here - of names no table knows - called
// it. The built-in's table now answers with the name of whatever
// customisation THIS program has compiled in, so those forms made a test's
// answer a matter of which machine built it; they are gone, the Document
// passes its own table, and these tests say what they always meant. No
// expectation changed.
const NO_RENAMES: &[RenamedSource] = &[];

// A rename table shaped like the built-in one: "branded_lines.4d" is an
// earlier name nobody's own file would have, "common.4d" a plain one
// anybody's might.
fn renamed_set() -> Vec<RenamedSource> {
    vec![
        renamed("branded_lines.4d", "linestyles.4d"),
        RenamedSource {
            distinctive: false,
            ..renamed("common.4d", "survey_codes_names.mapfile")
        },
    ]
}

// The two files the set is loaded as now, the library defining "A".
fn the_set_loaded(styles: &mut StyleLibrary) -> Vec<CustomisationSource> {
    add(styles, "A", "linestyles.4d");
    let mut loaded = Vec::new();
    record_customisation_load(
        &mut loaded,
        &[
            library("linestyles.4d"),
            mapfile("survey_codes_names.mapfile"),
        ],
        false,
        false,
    );
    loaded
}

mod customisation_record {
    use super::*;

    #[test]
    fn the_loaded_files_are_recorded_in_load_order() {
        let mut styles = StyleLibrary::default();
        add(&mut styles, "WATR Main", "linestyles.4d");
        add(&mut styles, "CULT Bollard", "symbols.4d");
        let mut loaded = Vec::new();
        record_customisation_load(
            &mut loaded,
            &[
                library("symbols.4d"),
                library("linestyles.4d"),
                mapfile("survey.mapfile"),
            ],
            false,
            false,
        );
        // In the order loaded, not the library's name order ("linestyles" <
        // "symbols").
        assert_eq!(
            customisation_record(&styles, &loaded),
            names(&["symbols.4d", "linestyles.4d", "survey.mapfile"])
        );
    }

    #[test]
    fn a_library_file_every_definition_of_which_was_replaced_is_not_recorded() {
        // extra.4d defined only "WATR Main", and a later mine.4d redefined it:
        // nothing is drawn with extra.4d any more, so a project does not need
        // it.
        let mut styles = StyleLibrary::default();
        add(&mut styles, "WATR Main", "mine.4d");
        let mut loaded = Vec::new();
        record_customisation_load(&mut loaded, &[library("extra.4d")], false, false);
        record_customisation_load(&mut loaded, &[library("mine.4d")], false, false);
        assert_eq!(customisation_record(&styles, &loaded), names(&["mine.4d"]));
    }

    #[test]
    fn a_replace_drops_the_earlier_files_of_the_kind_it_brought_and_no_other() {
        let mut styles = StyleLibrary::default();
        add(&mut styles, "A", "first.4d");
        let mut loaded = Vec::new();
        record_customisation_load(
            &mut loaded,
            &[library("first.4d"), mapfile("first.mapfile")],
            false,
            false,
        );
        // A replacing load that brought a mapfile only: the library stays.
        record_customisation_load(&mut loaded, &[mapfile("second.mapfile")], false, true);
        assert_eq!(
            customisation_record(&styles, &loaded),
            names(&["first.4d", "second.mapfile"])
        );
    }

    #[test]
    fn a_file_loaded_again_moves_to_where_it_was_loaded_last_and_is_listed_once() {
        let mut loaded = Vec::new();
        record_customisation_load(
            &mut loaded,
            &[mapfile("a.mapfile"), mapfile("b.mapfile")],
            false,
            false,
        );
        record_customisation_load(
            &mut loaded,
            &[mapfile("a.mapfile"), mapfile("a.mapfile")],
            false,
            false,
        );
        assert_eq!(loaded, vec![mapfile("b.mapfile"), mapfile("a.mapfile")]);
    }

    #[test]
    fn a_definition_from_a_file_no_load_accounts_for_is_still_recorded() {
        // The library was given some other way; what it is drawn with still
        // counts, after the loaded files and in name order.
        let mut styles = StyleLibrary::default();
        add(&mut styles, "B", "zeta.4d");
        add(&mut styles, "C", "alpha.4d");
        add(&mut styles, "D", "");
        assert_eq!(
            customisation_record(&styles, &[mapfile("m.mapfile")]),
            names(&["m.mapfile", "alpha.4d", "zeta.4d"]),
            "a definition made in a session names no file"
        );
    }

    #[test]
    fn opening_names_the_recorded_files_that_are_not_loaded_in_the_projects_order() {
        let mut styles = StyleLibrary::default();
        add(&mut styles, "A", "linestyles.4d");
        let loaded = vec![library("linestyles.4d")];
        let recorded = names(&[
            "survey.mapfile",
            "linestyles.4d",
            "symbols.4d",
            "survey.mapfile",
        ]);
        assert_eq!(
            customisation_not_loaded(&recorded, &styles, &loaded, NO_RENAMES),
            names(&["survey.mapfile", "symbols.4d"])
        );
        assert!(
            customisation_not_loaded(&names(&["linestyles.4d"]), &styles, &loaded, NO_RENAMES)
                .is_empty()
        );
    }

    // The reviewer's case: session 1 saved with test_symbols.4d loaded;
    // session 2 opens it without the file (warned) and saves unedited;
    // session 3 must still be warned. By hand: session 2 has only the
    // built-in's two files loaded, the open finds "test_symbols.4d" missing,
    // and nothing loads it before the save, so the record keeps it after the
    // session's own files.
    #[test]
    fn a_save_keeps_the_recorded_files_the_open_found_missing_and_no_load_brought_since() {
        let mut styles = StyleLibrary::default();
        add(&mut styles, "A", "linestyles.4d");
        let mut loaded = Vec::new();
        record_customisation_load(
            &mut loaded,
            &[library("linestyles.4d"), mapfile("survey.mapfile")],
            false,
            false,
        );
        let recorded = names(&["linestyles.4d", "test_symbols.4d", "survey.mapfile"]);
        let mut missing = customisation_not_loaded(&recorded, &styles, &loaded, NO_RENAMES);
        assert_eq!(missing, names(&["test_symbols.4d"]));
        assert_eq!(
            customisation_record_to_save(&recorded, &missing, &styles, &loaded, NO_RENAMES),
            names(&["linestyles.4d", "survey.mapfile", "test_symbols.4d"])
        );
        // An unrelated load judges nothing it did not bring.
        note_customisation_loaded(&mut missing, &[mapfile("other.mapfile")], NO_RENAMES);
        assert_eq!(missing, names(&["test_symbols.4d"]));
    }

    // Loaded after the open and then replaced away, the file was judged by
    // this session: the drawing is drawn without it here, and the record lets
    // it go. By hand: the open misses "old.4d"; loading it takes it off the
    // missing list; a library Replace with "new.4d" drops it from the loaded
    // files; the save records only what is loaded - "new.4d" (it still
    // defines "A").
    #[test]
    fn a_file_loaded_since_the_open_and_then_replaced_is_no_longer_recorded() {
        let mut styles = StyleLibrary::default();
        add(&mut styles, "A", "new.4d");
        let mut loaded = Vec::new();
        let recorded = names(&["old.4d"]);
        let mut missing = customisation_not_loaded(&recorded, &styles, &loaded, NO_RENAMES);
        assert_eq!(missing, names(&["old.4d"]));
        record_customisation_load(&mut loaded, &[library("old.4d")], false, false);
        note_customisation_loaded(&mut missing, &[library("old.4d")], NO_RENAMES);
        assert!(missing.is_empty());
        record_customisation_load(&mut loaded, &[library("new.4d")], true, false);
        assert_eq!(
            customisation_record_to_save(&recorded, &missing, &styles, &loaded, NO_RENAMES),
            names(&["new.4d"])
        );
    }

    // A missing list left from an earlier drawing keeps nothing the current
    // one never recorded: after NEW the record is empty, whatever the list
    // holds.
    #[test]
    fn a_missing_list_from_an_earlier_drawing_adds_nothing_to_a_new_one() {
        let styles = StyleLibrary::default();
        let loaded = vec![mapfile("survey.mapfile")];
        assert_eq!(
            customisation_record_to_save(
                &[],
                &names(&["test_symbols.4d"]),
                &styles,
                &loaded,
                NO_RENAMES
            ),
            names(&["survey.mapfile"])
        );
    }

    // A typed SAVE has somewhere to go with one project directory, or with
    // none once the drawing has a project - split as the interpreter splits
    // it. By hand: "SAVE" is 1 word, "SAVE x" 2, "SAVE \"my dir/x\"" 2 (the
    // quotes group), "SAVE a b" 3 (the interpreter's usage error), "SAVE \"x"
    // an unclosed quote the interpreter refuses.
    #[test]
    fn a_typed_save_has_a_destination_only_where_the_interpreter_would_save() {
        assert!(
            !typed_save_has_destination("SAVE", false),
            "untitled: nowhere to save"
        );
        assert!(typed_save_has_destination("SAVE", true));
        assert!(typed_save_has_destination("  save   ", true));
        assert!(typed_save_has_destination("SAVE x.katana", false));
        assert!(typed_save_has_destination(
            "SAVE \"C:/my dir/x.katana\"",
            false
        ));
        assert!(
            typed_save_has_destination("SAVE \"\"", false),
            "\"\" is a word to the interpreter"
        );
        assert!(
            !typed_save_has_destination("SAVE a b", true),
            "a usage error saves nothing"
        );
        assert!(!typed_save_has_destination("SAVE my dir/x.katana", false));
        assert!(
            !typed_save_has_destination("SAVE \"x.katana", true),
            "an unclosed quote"
        );
    }

    // A file named twice in one load is read once, however it is spelt. By
    // hand, in a fresh directory d holding x.mapfile and y.mapfile:
    // "d/x.mapfile", "d/sub/../x.mapfile" and "d/./x.mapfile" are one file;
    // "d/y.mapfile" is another; so two files, and two repeats, as named.
    #[test]
    fn a_file_named_twice_in_one_load_is_read_once() {
        let dir = std::env::temp_dir().join(format!(
            "katana-cad-tests-distinct-files-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(std::fs::create_dir_all(dir.join("sub")).is_ok());
        assert!(std::fs::write(dir.join("x.mapfile"), "x").is_ok());
        assert!(std::fs::write(dir.join("y.mapfile"), "y").is_ok());
        let x = dir.join("x.mapfile");
        let y = dir.join("y.mapfile");
        let x2 = dir.join("sub").join("..").join("x.mapfile");
        let x3 = dir.join(".").join("x.mapfile");
        let distinct =
            distinct_customisation_files(&[x.clone(), y.clone(), x2.clone(), x3.clone()]);
        // The temporary directory is absolute and in normal form already, so
        // the two names are their own absolute, lexically normal forms.
        let absolute = |path: &PathBuf| std::path::absolute(path).expect("an absolute path");
        assert_eq!(distinct.files, vec![absolute(&x), absolute(&y)]);
        assert_eq!(distinct.repeats, vec![x2, x3]);
        #[cfg(windows)]
        {
            // Windows names files without regard to case: X.MAPFILE is
            // x.mapfile.
            let upper = dir.join("X.MAPFILE");
            assert_eq!(
                distinct_customisation_files(&[x.clone(), upper])
                    .files
                    .len(),
                1
            );
        }
        // Two names for files that do not exist are not known to be one.
        assert_eq!(
            distinct_customisation_files(&[dir.join("a.4d"), dir.join("b.4d")])
                .files
                .len(),
            2
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ---- a file renamed since the project was saved ------------------------------------

    // The hash a former name is known by: FNV-1a over the bytes, 64-bit,
    // checked against the published test vectors of the algorithm.
    #[test]
    fn a_former_name_is_known_by_its_fnv1a64_bit_hash() {
        assert_eq!(source_name_hash(""), 0xcbf29ce484222325);
        assert_eq!(source_name_hash("a"), 0xaf63dc4c8601ec8c);
        assert_eq!(source_name_hash("foobar"), 0x85944171f73967e8);
    }

    // A project saved before its files were renamed records the old names;
    // the same files are loaded under the new ones. By hand: "old_codes.4d"
    // and "old_lines.4d" are renamed to the loaded survey_codes.mapfile and
    // linestyles.4d (which still defines "A"), so only "mine.4d" - which
    // nothing renamed and nothing loaded - is missing; and the save records
    // what is loaded, under the new names, and the one file still missing.
    #[test]
    fn a_recorded_former_name_is_answered_by_the_file_that_now_has_its_place() {
        let mut styles = StyleLibrary::default();
        add(&mut styles, "A", "linestyles.4d");
        let mut loaded = Vec::new();
        record_customisation_load(
            &mut loaded,
            &[library("linestyles.4d"), mapfile("survey_codes.mapfile")],
            false,
            false,
        );
        let table = vec![
            renamed("old_lines.4d", "linestyles.4d"),
            renamed("old_codes.4d", "survey_codes.mapfile"),
        ];
        let recorded = names(&["old_codes.4d", "old_lines.4d", "mine.4d"]);
        let missing = customisation_not_loaded(&recorded, &styles, &loaded, &table);
        assert_eq!(missing, names(&["mine.4d"]));
        // The save is given the table the open was, as one session gives
        // both.
        assert_eq!(
            customisation_record_to_save(&recorded, &missing, &styles, &loaded, &table),
            names(&["linestyles.4d", "survey_codes.mapfile", "mine.4d"])
        );
    }

    // The rename answers for a file only while the file that has its place is
    // loaded: without it, the drawing lacks what the old name brought, and the
    // warning names the file as the project recorded it.
    #[test]
    fn a_former_name_whose_file_is_not_loaded_now_is_missing_as_recorded() {
        let mut styles = StyleLibrary::default();
        add(&mut styles, "A", "mine.4d");
        let mut loaded = Vec::new();
        // linestyles.4d was loaded, and mine.4d then redefined all it brought:
        // nothing is drawn with it now.
        record_customisation_load(&mut loaded, &[library("linestyles.4d")], false, false);
        record_customisation_load(&mut loaded, &[library("mine.4d")], false, false);
        let table = vec![renamed("old_lines.4d", "linestyles.4d")];
        assert_eq!(
            customisation_not_loaded(&names(&["old_lines.4d"]), &styles, &loaded, &table),
            names(&["old_lines.4d"])
        );
        assert_eq!(
            customisation_not_loaded(&names(&["old_lines.4d"]), &styles, &[], &table),
            names(&["old_lines.4d"])
        );
    }

    // The built-in customisation's renames: eight, every earlier name answered
    // by the name the built-in declares, which the caller gives, in two sets
    // of four - the four first names, known only by hash, then the four
    // general ones, which can be spelt and are checked here against their own
    // hashes. Nothing here can spell the first four; that a rename is
    // answered is the tests above.
    #[test]
    fn the_built_ins_eight_earlier_names_are_each_answered_by_the_name_it_declares() {
        let renames = builtin_renames("Built In Test");
        assert_eq!(renames.len(), 8);
        let mut former = BTreeSet::new();
        for rename in &renames {
            assert_eq!(rename.now, "Built In Test");
            former.insert(rename.former_name);
            assert_ne!(rename.former_name, source_name_hash(&rename.now));
            assert_ne!(rename.former_name, source_name_hash(""));
        }
        assert_eq!(former.len(), renames.len(), "no two earlier names are one");

        // Two sets of four, each in the order the files loaded in.
        let general = [
            "linestyles.4d",
            "survey_codes.mapfile",
            "survey_codes_names.mapfile",
            "symbols.4d",
        ];
        for (i, name) in general.iter().enumerate() {
            assert_eq!(renames[i].set, 0, "{i}");
            assert_eq!(renames[4 + i].set, 1, "{i}");
            assert_eq!(renames[4 + i].former_name, source_name_hash(name), "{name}");
        }
    }

    // The case the table was extended for. A project saved while the built-in
    // was four files records their four general names; the built-in is one
    // customisation now, loaded under its own name. By hand: each of the four
    // is a plain name, and each stands beside three other names of its set,
    // so all four are answered by "Built In Test", which is loaded (it brought
    // rules, and definition "A" still comes from it) - nothing is missing, and
    // the save records the one name the drawing is drawn with now.
    #[test]
    fn a_record_of_the_four_general_names_is_answered_by_the_built_in_under_its_own_name() {
        let renames = builtin_renames("Built In Test");
        let recorded = names(&[
            "linestyles.4d",
            "survey_codes.mapfile",
            "survey_codes_names.mapfile",
            "symbols.4d",
        ]);
        let mut styles = StyleLibrary::default();
        add(&mut styles, "A", "Built In Test");
        let loaded = vec![whole("Built In Test")];

        let missing = customisation_not_loaded(&recorded, &styles, &loaded, &renames);
        assert!(missing.is_empty());
        assert_eq!(
            customisation_record_to_save(&recorded, &missing, &styles, &loaded, &renames),
            names(&["Built In Test"])
        );

        // Where the built-in is NOT loaded, what the four files brought is not
        // there, and the warning names them as the project recorded them.
        assert_eq!(
            customisation_not_loaded(&recorded, &StyleLibrary::default(), &[], &renames),
            recorded
        );
        let other = vec![whole("Council")];
        assert_eq!(
            customisation_not_loaded(&recorded, &StyleLibrary::default(), &other, &renames),
            recorded
        );

        // Opened without it and then given it: loading the built-in clears all
        // four, as loading the four files did.
        let mut at_open = recorded.clone();
        note_customisation_loaded(&mut at_open, &loaded, &renames);
        assert!(at_open.is_empty());
    }

    // A general name is one anybody's own file may have. By hand:
    // "symbols.4d" recorded alone has no other earlier name of its set beside
    // it, so no rename answers it; it is not loaded under its own name either,
    // so it is missing, and the save keeps it. Recorded twice it is still
    // alone. Beside ONE other general name it is the built-in's record, and
    // both are answered.
    #[test]
    fn a_lone_general_name_is_still_someones_own_file_and_two_together_are_the_built_ins() {
        let renames = builtin_renames("Built In Test");
        let mut styles = StyleLibrary::default();
        add(&mut styles, "A", "Built In Test");
        let loaded = vec![whole("Built In Test")];

        for recorded in [
            names(&["symbols.4d"]),
            names(&["symbols.4d", "symbols.4d"]),
            names(&["Built In Test", "symbols.4d"]),
        ] {
            let missing = customisation_not_loaded(&recorded, &styles, &loaded, &renames);
            assert_eq!(missing, names(&["symbols.4d"]));
            assert_eq!(
                customisation_record_to_save(&recorded, &missing, &styles, &loaded, &renames),
                names(&["Built In Test", "symbols.4d"])
            );
            // Loading the built-in does not bring a person's own file.
            let mut at_open = missing.clone();
            note_customisation_loaded(&mut at_open, &loaded, &renames);
            assert_eq!(at_open, names(&["symbols.4d"]));
        }

        let pair = names(&["symbols.4d", "linestyles.4d"]);
        assert!(customisation_not_loaded(&pair, &styles, &loaded, &renames).is_empty());
    }

    // Names of two SETS beside each other are evidence of neither: a record is
    // made at one time and holds the names of one set. By hand, with a table
    // of two plain earlier names in different sets, both renamed to the
    // loaded "now.json": each has no company of its own set, so neither is
    // answered - and put in ONE set, each is the other's company and both are.
    #[test]
    fn a_plain_name_is_answered_only_beside_an_earlier_name_of_its_own_set() {
        let styles = StyleLibrary::default();
        let loaded = vec![mapfile("now.json")];
        let recorded = names(&["first.4d", "second.4d"]);
        let table = |second_set: i32| {
            vec![
                RenamedSource {
                    distinctive: false,
                    set: 0,
                    ..renamed("first.4d", "now.json")
                },
                RenamedSource {
                    distinctive: false,
                    set: second_set,
                    ..renamed("second.4d", "now.json")
                },
            ]
        };
        assert_eq!(
            customisation_not_loaded(&recorded, &styles, &loaded, &table(1)),
            recorded
        );
        assert!(customisation_not_loaded(&recorded, &styles, &loaded, &table(0)).is_empty());
    }

    // A program with no built-in has no name to answer with, and its table
    // answers nothing - not even a distinctive earlier name. By hand: a table
    // whose one rename gives no name, the recorded earlier name, and a source
    // with nothing to do with it.
    #[test]
    fn a_rename_with_no_name_to_give_answers_nothing() {
        let styles = StyleLibrary::default();
        let loaded = vec![mapfile("other.json")];
        let nameless = vec![renamed("old.4d", "")];
        assert_eq!(
            customisation_not_loaded(&names(&["old.4d"]), &styles, &loaded, &nameless),
            names(&["old.4d"])
        );
        let mut at_open = names(&["old.4d"]);
        note_customisation_loaded(&mut at_open, &loaded, &nameless);
        assert_eq!(at_open, names(&["old.4d"]));

        // The table built for no built-in is such a table, all eight of it.
        for rename in builtin_renames("") {
            assert!(rename.now.is_empty());
        }
        let recorded = names(&[
            "linestyles.4d",
            "survey_codes.mapfile",
            "survey_codes_names.mapfile",
            "symbols.4d",
        ]);
        assert_eq!(
            customisation_not_loaded(&recorded, &styles, &loaded, &builtin_renames("")),
            recorded
        );
    }

    // ---- a plain earlier name, and loading what a warning asked for ----------------------

    // A plain earlier name recorded alone, or beside the names the set has
    // now, is a person's own file that happens to share it: missing while it
    // is not loaded, warned about, and kept by the save rather than dropped
    // for good.
    #[test]
    fn a_plain_former_name_outside_the_renamed_set_is_someone_elses_file_and_stays_missing() {
        let mut styles = StyleLibrary::default();
        let loaded = the_set_loaded(&mut styles);
        for recorded in [
            names(&["common.4d"]),
            names(&["linestyles.4d", "survey_codes_names.mapfile", "common.4d"]),
        ] {
            let missing = customisation_not_loaded(&recorded, &styles, &loaded, &renamed_set());
            assert_eq!(missing, names(&["common.4d"]));
            assert_eq!(
                customisation_record_to_save(&recorded, &missing, &styles, &loaded, &renamed_set()),
                names(&["linestyles.4d", "survey_codes_names.mapfile", "common.4d"])
            );
        }
    }

    // Beside a distinctive earlier name of the same table, the plain one is
    // the record of the renamed set, and the file that has its place answers
    // it.
    #[test]
    fn a_plain_former_name_beside_a_distinctive_one_is_answered_by_the_renamed_file() {
        let mut styles = StyleLibrary::default();
        let loaded = the_set_loaded(&mut styles);
        let recorded = names(&["branded_lines.4d", "common.4d"]);
        let missing = customisation_not_loaded(&recorded, &styles, &loaded, &renamed_set());
        assert!(missing.is_empty());
        assert_eq!(
            customisation_record_to_save(&recorded, &missing, &styles, &loaded, &renamed_set()),
            names(&["linestyles.4d", "survey_codes_names.mapfile"])
        );
    }

    // Opened with nothing loaded, a project that recorded the earlier names
    // is told they are missing; loading the files that have their places, as
    // the warning asks, clears them, and the save records the new names alone
    // - so the next open, anywhere, is about those and is cleared the same
    // way.
    #[test]
    fn loading_the_files_that_replaced_the_former_names_clears_what_the_open_found_missing() {
        let recorded = names(&["branded_lines.4d", "common.4d"]);
        let mut missing =
            customisation_not_loaded(&recorded, &StyleLibrary::default(), &[], &renamed_set());
        assert_eq!(missing, recorded);

        let mut styles = StyleLibrary::default();
        let loaded = the_set_loaded(&mut styles);
        note_customisation_loaded(&mut missing, &loaded, &renamed_set());
        assert!(missing.is_empty());
        let saved =
            customisation_record_to_save(&recorded, &missing, &styles, &loaded, &renamed_set());
        assert_eq!(
            saved,
            names(&["linestyles.4d", "survey_codes_names.mapfile"])
        );

        let mut next_open =
            customisation_not_loaded(&saved, &StyleLibrary::default(), &[], &renamed_set());
        assert_eq!(next_open, saved);
        note_customisation_loaded(&mut next_open, &loaded, &renamed_set());
        assert!(next_open.is_empty());

        // A person's own file of the plain name is not what the set's file is:
        // loading the set leaves it missing.
        let mut own = names(&["common.4d"]);
        note_customisation_loaded(&mut own, &loaded, &renamed_set());
        assert_eq!(own, names(&["common.4d"]));
    }

    // note_customisation_loaded sees only the names still missing. When the
    // open found only the plain earlier name missing - the distinctive one was
    // answered then - the load cannot tell it from someone's own file; the
    // save, which has the whole record, can, and does not keep it once its
    // file is loaded.
    #[test]
    fn the_save_judges_a_plain_former_name_against_the_whole_record() {
        let recorded = names(&["branded_lines.4d", "common.4d"]);
        let mut styles = StyleLibrary::default();
        add(&mut styles, "A", "linestyles.4d");
        let mut loaded = Vec::new();
        record_customisation_load(&mut loaded, &[library("linestyles.4d")], false, false);
        let mut missing = customisation_not_loaded(&recorded, &styles, &loaded, &renamed_set());
        assert_eq!(missing, names(&["common.4d"]));

        let load = vec![mapfile("survey_codes_names.mapfile")];
        record_customisation_load(&mut loaded, &load, false, false);
        note_customisation_loaded(&mut missing, &load, &renamed_set());
        assert_eq!(
            customisation_record_to_save(&recorded, &missing, &styles, &loaded, &renamed_set()),
            names(&["linestyles.4d", "survey_codes_names.mapfile"])
        );
    }

    // Of the four FIRST names only the second survey code file's was a plain
    // one; the other three carried the publisher's name and are distinctive.
    // Told by its place - third, the files' load order being linestyles, the
    // first survey codes, the second survey codes, symbols - and the four
    // general names are all plain: each is a name anyone's own file might
    // have.
    #[test]
    fn of_the_first_names_only_the_second_survey_code_files_is_plain_and_every_general_name_is() {
        let renames = builtin_renames("Built In Test");
        assert_eq!(renames.len(), 8);
        let plain: Vec<usize> = renames
            .iter()
            .enumerate()
            .filter(|(_, rename)| !rename.distinctive)
            .map(|(i, _)| i)
            .collect();
        assert_eq!(plain, vec![2, 4, 5, 6, 7]);
    }

    // ---- what a source brought -------------------------------------------------------------

    // One customisation brings definitions AND rules, and a Replace takes the
    // place of one kind at a time. By hand: "NSW" brought both; a Replace that
    // brought rules alone ("Council codes") takes the rules "NSW" brought and
    // leaves its definitions, so "NSW" is still a source, of definitions only -
    // and, bringing definitions alone now, it is recorded only while some
    // definition still comes from it. A second Replace, of definitions, leaves
    // it bringing nothing, and it goes.
    #[test]
    fn a_replace_of_one_kind_leaves_a_source_that_brought_both_with_the_other_kind() {
        let mut loaded = Vec::new();
        record_customisation_load(&mut loaded, &[whole("NSW")], false, false);
        record_customisation_load(&mut loaded, &[mapfile("Council codes")], false, true);
        assert_eq!(loaded, vec![library("NSW"), mapfile("Council codes")]);

        let mut from_nsw = StyleLibrary::default();
        add(&mut from_nsw, "A", "NSW");
        assert_eq!(
            customisation_record(&from_nsw, &loaded),
            names(&["NSW", "Council codes"])
        );
        let mut from_elsewhere = StyleLibrary::default();
        add(&mut from_elsewhere, "A", "mine");
        assert_eq!(
            customisation_record(&from_elsewhere, &loaded),
            names(&["Council codes", "mine"]),
            "its rules were replaced and none of its definitions is left"
        );

        record_customisation_load(&mut loaded, &[library("Council symbols")], true, false);
        assert_eq!(
            loaded,
            vec![mapfile("Council codes"), library("Council symbols")]
        );
    }

    // A source that brought both is taken at its word for its rules, which
    // carry no source name: it is recorded although no definition comes from
    // it.
    #[test]
    fn a_source_that_brought_rules_is_recorded_whatever_became_of_its_definitions() {
        let mut styles = StyleLibrary::default();
        add(&mut styles, "A", "mine");
        assert_eq!(
            customisation_record(&styles, &[whole("NSW")]),
            names(&["NSW", "mine"])
        );
        assert_eq!(
            customisation_record(&styles, &[library("NSW")]),
            names(&["mine"])
        );
    }

    // Loaded again in a merge, a source still brings what it brought before:
    // the definitions of the first load are still there. By hand: "A" brought
    // both, then a file of the same name brings rules alone - one source, both
    // kinds, with the notice the earlier load gave, the later one giving none.
    #[test]
    fn a_source_loaded_again_still_brings_what_it_brought_before_and_keeps_its_notice() {
        let mut loaded = Vec::new();
        let mut first = whole("A");
        first.notice = names(&["Not for resale."]);
        record_customisation_load(&mut loaded, &[first.clone(), mapfile("B")], false, false);
        record_customisation_load(&mut loaded, &[mapfile("A")], false, false);
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0], mapfile("B"));
        assert_eq!(
            loaded[1], first,
            "moved to the end, still bringing both, notice kept"
        );
    }

    // A table of colours brings neither kind. No Replace takes its place, and
    // a project drawn with its colours records it.
    #[test]
    fn a_source_that_brought_neither_kind_outlives_a_replace_and_is_recorded() {
        let colours = source("Site colours", false, false);
        let mut loaded = Vec::new();
        record_customisation_load(&mut loaded, &[colours.clone(), whole("NSW")], false, false);
        record_customisation_load(&mut loaded, &[whole("Council")], true, true);
        assert_eq!(loaded, vec![colours, whole("Council")]);
        assert_eq!(
            customisation_record(&StyleLibrary::default(), &loaded),
            names(&["Site colours", "Council"])
        );
    }

    // ---- a definition renamed since the drawing was saved ----------------------------------

    // A drawing saved before a definition was renamed names it the earlier
    // way. A lookup that misses asks for the name it has now, and looks that
    // up instead. Names compare exactly, as the library's do.
    #[test]
    fn a_definition_lookup_that_misses_is_retried_under_the_name_it_has_now() {
        let table = [RenamedDefinition {
            former_name: source_name_hash("Brand SURVEY - FU"),
            now: "SURVEY - FU",
        }];
        assert_eq!(
            definition_name_now_in("Brand SURVEY - FU", &table),
            "SURVEY - FU"
        );
        assert!(definition_name_now_in("brand SURVEY - FU", &table).is_empty());
        assert!(definition_name_now_in("SURVEY - FU", &table).is_empty());
        assert!(definition_name_now_in("", &table).is_empty());

        let mut styles = StyleLibrary::default();
        add(&mut styles, "SURVEY - FU", "linestyles.4d");
        assert!(styles.find("Brand SURVEY - FU").is_none());
        let found = styles
            .find(definition_name_now_in("Brand SURVEY - FU", &table))
            .expect("the definition under its name now");
        assert_eq!(found.name, "SURVEY - FU");
    }

    // The built-in table: 29 definitions, each earlier name known once, none
    // the hash of a name given now (a lookup of a name as it is now never
    // needs the table), and no name given now renamed again.
    #[test]
    fn the_built_in_definition_renames_are_twenty_nine_distinct_names_in_byte_order() {
        let renames = builtin_definition_renames();
        assert_eq!(renames.len(), 29);
        let mut former = BTreeSet::new();
        let mut now = BTreeSet::new();
        for rename in renames {
            former.insert(rename.former_name);
            now.insert(rename.now);
            assert!(!rename.now.is_empty());
            assert!(definition_name_now(rename.now).is_empty(), "{}", rename.now);
        }
        assert_eq!(former.len(), renames.len());
        assert_eq!(now.len(), renames.len());
        assert!(renames.windows(2).all(|pair| pair[0].now <= pair[1].now));
    }
}
