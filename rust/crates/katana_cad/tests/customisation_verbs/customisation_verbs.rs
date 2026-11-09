//! CUSTOMISE (customisation_verbs) through CommandInterpreter::run, on a Document of the test's
//! own: what every word does, what each refuses, and that a refused line leaves the session and
//! the files as they were (tests/cad/customisation/test_customisation_verbs.cpp).
//!
//! The customisations loaded here and every count worked from them are at the top of
//! verbs_support.rs. The one case that changes the working folder,
//! CustomisationVerbs.AKeywordIsTheWholeFirstWordAndAFileSoNamedIsGivenWithItsDirectory, is in the
//! target tests/customisation_verbs_env: the working folder is the process's, and this binary runs
//! its tests on parallel threads.

use std::fs;
use std::path::Path;

use katana_cad::customisation_host::{
    BuiltInCustomisation, BuiltInEntry, built_in_advice_for, built_ins_of,
    compiled_in_customisation, start_customisation,
};
use katana_cad::customisation_state::CustomisationOrigin;
use katana_cad::customisation_verbs::customisation_verb_help;
use katana_cad::definition_edit::remove_definition_line;
use katana_cad::definition_users::definition_users;
use katana_cad::{CommandInterpreter, Document};
use katana_commands::create_style;
use katana_core::ErrorCode;
use katana_core::text::ReplyRecord;
use katana_entity::style_library::add_or_replace;
use katana_entity::{
    ColourTable, Customisation, LineStyle, Stroke, StrokeOp, Style, SurveyRule, SurveySection,
};

use crate::customisation_support::{Scratch, built_in_from, fnv1a};
use crate::must;
use crate::verbs_support::{
    BUILT_IN, Bare, DEFAULT_LINEWORK, Hosted, LEGACY_LIBRARY, MARKS, NO_STYLES, OWN, SITE, TINTS,
    add_definition, beside, contents_of, field, parsed, record, records_of, shown, source,
    two_built_ins, typed,
};

// A file that holds a little of everything, for what DEFINITIONS takes and
// what it leaves: one symbol, one colour, one rule, a spelling of Start, a
// switch, and two sources - Mixed, which brought the definition, and Rulers,
// which brought rules alone.
const MIXED: &str = r##"{
  "format": "katana-customisation", "version": 1, "name": "Mixed",
  "sources": [
    {"name": "Mixed", "definitions": true, "notice": ["Mixed: free to use."]},
    {"name": "Rulers", "rules": true, "notice": ["Rulers: not for the symbol library."]}
  ],
  "colours": {"sui mix": "#102030"},
  "linework": {"start": "S"},
  "automation": {"codesOnSurveyImport": false},
  "symbols": [
    {"name": "MIX Dot", "atVertices": true, "strokes": [["circle", 1]]}
  ],
  "codes": [
    {"key": "MX*", "sets": "feature", "layer": "MIX"}
  ]
}"##;

#[track_caller]
fn make_folder_of(file: &Path) {
    let folder = file.parent().expect("a file is in a folder");
    fs::create_dir_all(folder).expect("the folder is made");
}

#[track_caller]
fn write_file(path: &Path, text: &str) {
    fs::write(path, text).expect("the file is written");
}

// The record EXPORT replied with, and the customisation the file it wrote reads as.
#[track_caller]
fn exported_to(session: &mut Hosted, name: &str, words: &str) -> (ReplyRecord, Customisation) {
    let file = session.scratch.root.join(name);
    let line = format!("CUSTOMISE EXPORT {} {words}", typed(&file));
    let reply = session.ok(&line);
    (record(&reply, "exported", 0), parsed(&contents_of(&file)))
}

// The customisation a part EXPORT writes reads as.
#[track_caller]
fn part(session: &mut Hosted, name: &str, words: &str) -> Customisation {
    let file = session.scratch.root.join(name);
    let line = format!("CUSTOMISE EXPORT {} {words}", typed(&file));
    session.ok(&line);
    parsed(&contents_of(&file))
}

// The C++ sets fs::perms owner_read | group_read | others_read in the place of
// what the file had; Windows has one flag for all of it.
#[cfg(unix)]
#[track_caller]
fn make_read_only(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o444))
        .expect("the kept file is made read-only");
}

#[cfg(windows)]
#[track_caller]
fn make_read_only(path: &Path) {
    let mut permissions = fs::metadata(path)
        .expect("the kept file is there")
        .permissions();
    permissions.set_readonly(true);
    fs::set_permissions(path, permissions).expect("the kept file is made read-only");
}

#[cfg(unix)]
fn owner_may_write(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path).is_ok_and(|metadata| metadata.permissions().mode() & 0o200 != 0)
}

#[cfg(windows)]
fn owner_may_write(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|metadata| !metadata.permissions().readonly())
}

// fs::perms::owner_write added to what the file has.
#[cfg(unix)]
#[track_caller]
fn let_owner_write(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let mode = fs::metadata(path)
        .expect("the kept file is there")
        .permissions()
        .mode();
    fs::set_permissions(path, fs::Permissions::from_mode((mode & 0o7777) | 0o200))
        .expect("the kept file may be written again");
}

#[cfg(windows)]
#[allow(
    clippy::permissions_set_readonly_false,
    reason = "Windows has no other write permission to give back: this is the C++'s owner_write \
              added."
)]
#[track_caller]
fn let_owner_write(path: &Path) {
    let mut permissions = fs::metadata(path)
        .expect("the kept file is there")
        .permissions();
    permissions.set_readonly(false);
    fs::set_permissions(path, permissions).expect("the kept file may be written again");
}

// A style made as a file's are: no verb checks what a style names.
#[track_caller]
fn add_style(session: &mut Hosted, name: &str, linetype: &str, symbol: &str) {
    let made = Style {
        name: String::from(name),
        linetype: String::from(linetype),
        symbol: String::from(symbol),
        ..Style::default()
    };
    must(session.document.execute(create_style(made)));
}

mod customisation_verbs {
    use super::*;

    // ---- the report ------------------------------------------------------------------------

    #[test]
    fn alone_it_reports_the_counts_then_records_then_the_coverage() {
        let mut session = Hosted::new("report");
        session.start();
        // 2 definitions in no group: TEST Peg is offered as a symbol (at
        // vertices, and listed as one), TEST Fence as a linestyle.
        let expected = String::from(concat!(
            "2 linestyle and symbol definitions in 0 groups: 1 offered as symbols, 1 as ",
            "linestyles (one definition can be both)\n",
            "2 survey code rules over 2 distinct codes\n",
            "customisation name=\"Small Built In\" origin=builtIn kept=yes definitions=2 ",
            "codes=2 rules=2 colours=1\n",
            "source name=\"Small Built In\" definitions=yes rules=yes\n",
            "automation auto.codes=on auto.linework=on\n",
        )) + DEFAULT_LINEWORK
            + "\n"
            + NO_STYLES
            + "\n"
            + "Kept file: "
            + &shown(&session.kept_file)
            + ", not written yet; CUSTOMISE KEEP writes it.";
        assert_eq!(session.ok("CUSTOMISE"), expected);
        // The interpreter reads the verb as it reads every verb: in any case,
        // and by its other spelling.
        assert_eq!(session.ok("customise"), expected);
        assert_eq!(session.ok("CUSTOMIZE"), expected);
    }

    // `kept=yes` in the record says only that the session is what the next start
    // gives. With no kept file that is trivially so, and reads as a promise of a
    // file that is not there; the sentence at the end of the report says whether
    // there is one, and where.
    #[test]
    fn the_report_says_plainly_whether_a_kept_file_exists_and_where() {
        // No kept file at all: the program was given none.
        {
            let mut session = Hosted::with("report-none", true, false);
            session.start();
            let reply = session.ok("CUSTOMISE");
            assert!(reply.contains(" origin=builtIn kept=yes "), "{reply}");
            assert!(
                reply.ends_with(concat!(
                    "Kept file: none. This session keeps no file, so each start gives the ",
                    "built-in customisation (kept=yes then means only that nothing is waiting to ",
                    "be kept); the environment variable KATANA_CUSTOMISATION names a file to keep ",
                    "one in."
                )),
                "{reply}"
            );
        }
        // A kept file named and not there yet, then there once KEEP has written it.
        {
            let mut session = Hosted::new("report-kept");
            session.start();
            let not_yet = format!(
                "Kept file: {}, not written yet; CUSTOMISE KEEP writes it.",
                shown(&session.kept_file)
            );
            assert!(session.ok("CUSTOMISE").ends_with(&not_yet));
            session.ok("CUSTOMISE SET auto.codes=off");
            session.ok("CUSTOMISE KEEP");
            let there = format!(
                "Kept file: {}, which the next start reads.",
                shown(&session.kept_file)
            );
            assert!(session.ok("CUSTOMISE").ends_with(&there));
        }
        // A session given no host says nothing of a kept file: it has no notion of one.
        {
            let mut session = Bare::default();
            assert!(!session.ok("CUSTOMISE").contains("Kept file"));
        }
    }

    // A kept file that is there and does not read is not one "the next start reads":
    // the start falls back to the built-in, so the sentence says so, and that KEEP
    // is what sets it aside.
    #[test]
    fn the_report_does_not_say_the_next_start_reads_a_kept_file_that_does_not_read() {
        for bytes in ["{ not json", ""] {
            let mut session = Hosted::new("report-bad");
            make_folder_of(&session.kept_file);
            write_file(&session.kept_file, bytes);
            session.start();
            let reply = session.ok("CUSTOMISE");
            let expected = format!(
                "Kept file: {}, which is not a customisation that reads, so the next start does \
                 not use it; CUSTOMISE KEEP sets it aside as .bad.",
                shown(&session.kept_file)
            );
            assert!(reply.ends_with(&expected), "{bytes:?}: {reply}");
            assert!(
                !reply.contains("which the next start reads"),
                "{bytes:?}: {reply}"
            );
        }
        // A folder of the kept file's name cannot be read at all, and KEEP does not move it.
        let mut folder = Hosted::new("report-folder");
        fs::create_dir_all(&folder.kept_file).expect("the folder is made");
        folder.start();
        let expected = format!(
            "Kept file: {}, which cannot be read, so the next start does not use it.",
            shown(&folder.kept_file)
        );
        assert!(folder.ok("CUSTOMISE").ends_with(&expected));
    }

    #[test]
    fn with_nothing_loaded_it_says_so_and_how_to_load() {
        let mut session = Bare::default();
        assert_eq!(
            session.ok("CUSTOMISE"),
            String::from(concat!(
                "No customisation is loaded.\n",
                "  CUSTOMISE <file> [<file>...]  loads Katana customisation files\n",
                "customisation name=\"\" origin=none kept=no definitions=0 codes=0 ",
                "rules=0 colours=0\n",
                "automation auto.codes=on auto.linework=on\n",
            )) + DEFAULT_LINEWORK
        );
    }

    #[test]
    fn a_customisation_that_brings_neither_kind_is_not_said_to_be_no_customisation() {
        // A file of settings alone: a name, the seven control codes, the two
        // switches. Loaded, a customisation IS loaded - the record below the
        // first line names it and says where it came from - so the first line may
        // not say that none is. What is true of it is that nothing in it draws.
        const SETTINGS: &str = r#"{
  "format": "katana-customisation", "version": 1, "name": "Set",
  "linework": {"start": "S", "end": "E", "close": "C", "arcStart": "PC", "arcEnd": "PT",
               "join": "J", "rectangle": "R"},
  "automation": {"codesOnSurveyImport": false, "lineworkOnSurveyImport": false}
}"#;
        let scratch = Scratch::new("settings-only");
        let mut session = Bare::default();
        let line = format!(
            "CUSTOMISE {}",
            typed(&scratch.write("settings.json", SETTINGS))
        );
        session.ok(&line);
        assert_eq!(
            session.ok("CUSTOMISE"),
            concat!(
                "No linestyle or symbol definitions and no survey code rules are loaded.\n",
                "  CUSTOMISE <file> [<file>...]  loads Katana customisation files\n",
                "customisation name=Set origin=loaded kept=no definitions=0 codes=0 rules=0 ",
                "colours=0\n",
                "source name=Set definitions=no rules=no\n",
                "automation auto.codes=off auto.linework=off\n",
                "linework linework.start=S linework.end=E linework.close=C linework.arcstart=PC ",
                "linework.arcend=PT linework.join=J linework.rectangle=R"
            )
        );
    }

    #[test]
    fn json_is_the_same_as_one_object() {
        let mut session = Hosted::new("json");
        session.start();
        // Keys in alphabetical order, two blanks a level. By hand from BUILT_IN:
        // neither rule has anything wrong with it, the drawing has no styles, and
        // the session is based on the built-in itself, by the digest of its bytes.
        // `start`, the last member, is what the start found: the built-in read and
        // nothing was kept, so nothing went wrong and nothing was made from
        // another built-in. (The member was added on 2026-10-07; before it the
        // object ended at `sources`.) `builtIns` lists what the host offers: the
        // one built-in, the default and the one the session is, with the counts of
        // BUILT_IN (2 definitions, 2 rules over 2 codes) and no file it was
        // compiled in from.
        let expected = String::from(
            r##"{
  "automation": {
    "codesOnSurveyImport": true,
    "lineworkOnSurveyImport": true
  },
  "basedOn": {
    "digest": ""##,
        ) + &fnv1a(BUILT_IN)
            + r##"",
    "name": "Small Built In"
  },
  "builtIn": "Small Built In",
  "builtIns": [
    {
      "active": true,
      "codes": 2,
      "default": true,
      "definitions": 2,
      "description": "Two definitions and two rules, for tests",
      "name": "Small Built In",
      "rules": 2,
      "source": ""
    }
  ],
  "colours": {
    "sui gas": "#FF7F00"
  },
  "counts": {
    "codes": 2,
    "colours": 1,
    "definitions": 2,
    "groups": 0,
    "linestyles": 1,
    "rules": 2,
    "symbols": 1
  },
  "coverage": {
    "builtIn": 0,
    "named": 0,
    "notLinestyles": [],
    "resolved": 0,
    "styles": 0,
    "unresolved": []
  },
  "description": "Two definitions and two rules, for tests",
  "kept": true,
  "linework": {
    "arcEnd": "EC",
    "arcStart": "BC",
    "close": "CL",
    "end": "END",
    "join": "JPN",
    "rectangle": "RECT",
    "start": "ST"
  },
  "missing": [],
  "name": "Small Built In",
  "notice": [
    "Written for these tests."
  ],
  "origin": "builtIn",
  "problems": {
    "byKind": {},
    "cannotApply": 0,
    "rules": 2,
    "undefined": [],
    "warnings": 0
  },
  "sources": [
    {
      "definitions": true,
      "name": "Small Built In",
      "notice": [],
      "rules": true
    }
  ],
  "start": {
    "keptFromAnotherBuiltIn": false,
    "problems": []
  }
}"##;
        assert_eq!(session.ok("CUSTOMISE JSON"), expected);
        assert_eq!(session.ok("customise json"), expected);
    }

    #[test]
    fn json_counts_what_is_wrong_with_the_rules_without_the_word_a_script_fails_on() {
        // Site alone: its GT* rule names a linestyle nothing defines (one
        // "unresolved linestyle" warning, and one undefined name).
        let scratch = Scratch::new("json-problems");
        let mut session = Bare::default();
        let line = format!("CUSTOMISE {}", typed(&scratch.write("site.json", SITE)));
        session.ok(&line);
        let json = session.ok("CUSTOMISE JSON");
        assert!(
            json.contains("\"byKind\": {\n      \"unresolved linestyle\": 1\n    }"),
            "{json}"
        );
        assert!(json.contains("\"cannotApply\": 0"), "{json}");
        assert!(
            json.contains("\"undefined\": [\n      \"SITE Gate\"\n    ]"),
            "{json}"
        );
        assert!(json.contains("\"warnings\": 1"), "{json}");
        assert!(json.contains("\"origin\": \"loaded\""), "{json}");
        assert!(json.contains("\"basedOn\": null"), "{json}");
    }

    #[test]
    fn json_counts_what_the_drawing_uses_and_carries_each_sources_notice_and_what_is_missing() {
        // The built-in with Marks merged in - a source with a notice of its own -
        // and a drawing of four styles:
        //
        //   Fence   linetype TEST Fence, which the library defines
        //   Ghost   linetype TEST Spare, a definition removed again from under it
        //   Mark    symbol cross, a shape Katana draws itself
        //   Plain   names nothing
        //
        // So 4 styles; 2 name a definition (Fence and Ghost) and 1 of those is
        // defined; 1 is drawn by Katana itself; TEST Spare is the one name the
        // styles give that nothing defines.
        let mut session = Hosted::new("json-coverage");
        session.start();
        let line = format!(
            "CUSTOMISE {}",
            typed(&session.scratch.write("marks.json", MARKS))
        );
        session.ok(&line);
        add_definition(&mut session.document, "TEST Spare", false);
        session.ok("STYLE NEW Fence");
        session.ok("STYLE SET Fence linetype \"TEST Fence\"");
        session.ok("STYLE NEW Ghost");
        session.ok("STYLE SET Ghost linetype \"TEST Spare\"");
        session.ok("STYLE NEW Mark");
        session.ok("STYLE SET Mark symbol cross");
        session.ok("STYLE NEW Plain");
        session.ok("CUSTOMISE REMOVE \"TEST Spare\" FORCE");

        let json = session.ok("CUSTOMISE JSON");
        assert!(
            json.contains(
                r#"  "coverage": {
    "builtIn": 1,
    "named": 2,
    "notLinestyles": [],
    "resolved": 1,
    "styles": 4,
    "unresolved": [
      "TEST Spare"
    ]
  },"#
            ),
            "{json}"
        );
        // Each source with its own notice: the built-in's is the session's, said
        // at the top, so its entry has none.
        assert!(
            json.contains(
                r#"  "sources": [
    {
      "definitions": true,
      "name": "Small Built In",
      "notice": [],
      "rules": true
    },
    {
      "definitions": true,
      "name": "Marks",
      "notice": [
        "Marks: drawn by hand."
      ],
      "rules": true
    }
  ]"#
            ),
            "{json}"
        );
        assert!(json.contains("  \"missing\": [],\n"), "{json}");

        // The project is recorded with the two customisations it is drawn with,
        // in load order. Opened where neither is loaded, both are missing.
        let project = session.scratch.root.join("drawn.katana");
        must(session.document.save_as(&project));
        let mut other = Bare::default();
        must(other.document.open(&project));
        assert!(other.ok("CUSTOMISE JSON").contains(
            r#"  "missing": [
    "Small Built In",
    "Marks"
  ],"#
        ));
        assert!(
            other
                .ok("CUSTOMISE")
                .contains("missing name=\"Small Built In\"\nmissing name=Marks")
        );
    }

    // ---- loading ---------------------------------------------------------------------------

    #[test]
    fn a_file_is_merged_into_what_is_loaded_and_the_reply_says_what_it_did() {
        let mut session = Hosted::new("merge");
        session.start();
        let site = session.scratch.write("site.json", SITE);
        let reply = session.ok(&format!("CUSTOMISE {}", typed(&site)));

        let loaded = record(&reply, "loaded", 0);
        assert_eq!(field(&loaded, "file"), shown(&site));
        assert_eq!(field(&loaded, "name"), "Site");
        assert_eq!(field(&loaded, "definitions_added"), "1"); // SITE Tree
        assert_eq!(field(&loaded, "definitions_replaced"), "1"); // TEST Peg
        assert_eq!(field(&loaded, "codes_added"), "2"); // TR* symbol, GT* feature
        assert_eq!(field(&loaded, "codes_replaced"), "1"); // FE* feature
        assert_eq!(field(&loaded, "colours_added"), "0");
        assert_eq!(field(&loaded, "colours_replaced"), "0");
        assert_eq!(field(&loaded, "linework"), "no");
        assert_eq!(field(&loaded, "automation"), "no");
        // A merge removes nothing, and says nothing of removing.
        assert_eq!(records_of(&reply, "removed"), 0, "{reply}");
        // What the rules ask for that nothing defines, then what is loaded now.
        assert!(
            reply.ends_with(concat!(
                "undefined names=1\n",
                "undefined name=\"SITE Gate\"\n",
                "customisation name=\"Small Built In\" origin=loaded kept=no ",
                "definitions=3 codes=4 rules=4 colours=1"
            )),
            "{reply}"
        );

        // The session is what the reply says: the fence's rule is Site's now, the
        // peg's is still the built-in's, and it keeps the name it had.
        assert_eq!(session.document.style_library().size(), 3);
        let rules = session.document.survey_map().rules();
        assert_eq!(rules.len(), 4);
        assert_eq!(rules[0].key, "FE*");
        assert_eq!(rules[0].model, "SITE FENCES");
        assert_eq!(rules[1].key, "PG*");
        assert_eq!(rules[2].key, "TR*");
        assert_eq!(rules[3].key, "GT*");
        assert_eq!(session.state().name, "Small Built In");
        assert_eq!(session.state().origin, CustomisationOrigin::Loaded);
        assert!(!session.state().kept);
        assert_eq!(session.state().sources.len(), 2);
        assert_eq!(session.state().sources[1].name, "Site");
    }

    #[test]
    fn replace_takes_the_place_of_each_kind_and_says_what_is_gone() {
        let mut session = Hosted::new("replace");
        session.start();
        let site = session.scratch.write("site.json", SITE);
        let reply = session.ok(&format!("customise replace {}", typed(&site)));

        let loaded = record(&reply, "loaded", 0);
        assert_eq!(field(&loaded, "file"), shown(&site));
        assert_eq!(field(&loaded, "definitions_added"), "1");
        assert_eq!(field(&loaded, "definitions_replaced"), "1");
        assert_eq!(field(&loaded, "codes_added"), "2");
        assert_eq!(field(&loaded, "codes_replaced"), "1");
        // TEST Fence and PG* were the built-in's and Site does not bring them.
        // Site brought both kinds, so nothing of the built-in's definitions or
        // rules is left to be named by, and the session takes Site's name. The
        // colour table is merged by name in either mode: sui gas stays.
        assert!(
            reply.ends_with(concat!(
                "removed definitions=1 codes=1\n",
                "removed definition=\"TEST Fence\"\n",
                "removed code=PG*\n",
                "undefined names=1\n",
                "undefined name=\"SITE Gate\"\n",
                "customisation name=Site origin=loaded kept=no definitions=2 ",
                "codes=3 rules=3 colours=1"
            )),
            "{reply}"
        );
        assert!(!session.document.style_library().contains("TEST Fence"));
        assert_eq!(session.document.survey_map().size(), 3);
        assert_eq!(session.state().sources.len(), 1);
        assert_eq!(session.state().sources[0].name, "Site");

        assert_eq!(
            session.refused("CUSTOMISE REPLACE").message,
            "usage: CUSTOMISE [REPLACE] <file> [<file>...]"
        );
    }

    #[test]
    fn definitions_merges_the_definitions_and_colours_and_leaves_the_rules_alone() {
        let mut session = Hosted::new("definitions");
        session.start();
        let site = session.scratch.write("site.json", SITE);
        let reply = session.ok(&format!("CUSTOMISE DEFINITIONS {}", typed(&site)));

        // Site's two symbols: SITE Tree is new and TEST Peg takes the place of
        // the built-in's. Its three rules are not taken: the built-in's two are
        // as they were (FE* still lays FENCES), and the reply counts the three
        // left, as one record that says what else it held none of.
        let loaded = record(&reply, "loaded", 0);
        assert_eq!(field(&loaded, "name"), "Site");
        assert_eq!(field(&loaded, "definitions_added"), "1");
        assert_eq!(field(&loaded, "definitions_replaced"), "1");
        assert_eq!(field(&loaded, "codes_added"), "0");
        assert_eq!(field(&loaded, "codes_replaced"), "0");
        let left = record(&reply, "left", 0);
        assert_eq!(field(&left, "rules"), "3");
        assert_eq!(field(&left, "linework"), "no");
        assert_eq!(field(&left, "automation"), "no");
        assert!(
            reply.ends_with(concat!(
                "customisation name=\"Small Built In\" origin=loaded kept=no ",
                "definitions=3 codes=2 rules=2 colours=1"
            )),
            "{reply}"
        );
        assert_eq!(session.document.survey_map().rules()[0].model, "FENCES");
        assert!(!session.state().kept);
    }

    #[test]
    fn definitions_takes_the_colours_and_not_the_control_codes_the_switches_or_a_rules_only_source()
    {
        let mut session = Hosted::new("definitions-mixed");
        session.start();
        let mixed = session.scratch.write("mixed.json", MIXED);
        let reply = session.ok(&format!("CUSTOMISE DEFINITIONS {}", typed(&mixed)));

        let loaded = record(&reply, "loaded", 0);
        assert_eq!(field(&loaded, "definitions_added"), "1"); // MIX Dot
        assert_eq!(field(&loaded, "colours_added"), "1"); // sui mix
        assert_eq!(field(&loaded, "codes_added"), "0");
        // What it held and was left: the one rule, the spelling of Start and the
        // switch.
        let left = record(&reply, "left", 0);
        assert_eq!(field(&left, "rules"), "1");
        assert_eq!(field(&left, "linework"), "yes");
        assert_eq!(field(&left, "automation"), "yes");

        assert_eq!(session.document.style_library().size(), 3);
        assert_eq!(session.state().colours.size(), 2, "sui gas and sui mix");
        assert_eq!(session.state().linework.start, "ST");
        assert!(session.state().automation.codes_on_survey_import);
        assert_eq!(session.document.survey_map().size(), 2);
        // Mixed is a source of the session, as one that brought definitions
        // alone and with its notice; Rulers brought rules only, and none was
        // taken, so it is none of the session's.
        let sources = &session.state().sources;
        let mixed = sources
            .iter()
            .find(|each| each.name == "Mixed")
            .expect("Mixed is a source");
        assert!(mixed.definitions);
        assert!(!mixed.rules);
        assert_eq!(mixed.notice, ["Mixed: free to use."]);
        assert!(!sources.iter().any(|each| each.name == "Rulers"));
    }

    #[test]
    fn definitions_of_a_file_with_no_definition_or_colour_is_refused_and_changes_nothing() {
        let mut session = Hosted::new("definitions-none");
        session.start();
        // A file of rules alone: nothing to take, and the refusal says where its
        // rules are loaded.
        const RULES_ONLY: &str = r#"{
  "format": "katana-customisation", "version": 1, "name": "Rules Only",
  "codes": [{"key": "RU*", "sets": "feature", "layer": "RULES"}]
}"#;
        let rules = session.scratch.write("rules.json", RULES_ONLY);
        let error = session.refused(&format!("CUSTOMISE DEFINITIONS {}", typed(&rules)));
        assert_eq!(error.code, ErrorCode::NotFound);
        assert_eq!(
            error.message,
            concat!(
                "CUSTOMISE DEFINITIONS: the file holds no definitions or colours to load, so ",
                "nothing was loaded; its survey code rules are loaded by CUSTOMISE <file>"
            )
        );
        assert!(session.state().kept, "a refused line changes nothing");
        assert_eq!(session.document.survey_map().size(), 2);

        // A file that is not one, and a file that is not there, are refused as
        // any load is.
        let old = session.scratch.write("old.txt", LEGACY_LIBRARY);
        assert!(
            session
                .refused(&format!("CUSTOMISE DEFINITIONS {}", typed(&old)))
                .message
                .contains("not a Katana customisation file")
        );
        let absent = session.scratch.root.join("absent.json");
        assert_eq!(
            session
                .refused(&format!("CUSTOMISE DEFINITIONS {}", typed(&absent)))
                .code,
            ErrorCode::NotFound
        );
        // With no file there is a usage, in its own words.
        assert_eq!(
            session.refused("CUSTOMISE DEFINITIONS").message,
            "usage: CUSTOMISE DEFINITIONS <file> [<file>...]"
        );
        assert!(session.state().kept);
    }

    #[test]
    fn a_file_named_twice_is_read_once_and_said() {
        let mut session = Hosted::new("twice");
        session.start();
        let site = session.scratch.write("site.json", SITE);
        let reply = session.ok(&format!("CUSTOMISE {} {}", typed(&site), typed(&site)));
        assert_eq!(records_of(&reply, "repeated"), 1, "{reply}");
        assert_eq!(field(&record(&reply, "repeated", 0), "file"), shown(&site));
        assert_eq!(records_of(&reply, "loaded"), 1, "{reply}");
        // Read twice, each of Site's three rules would be in the map twice: 7.
        assert_eq!(session.document.survey_map().size(), 4);
    }

    #[test]
    fn a_reply_counts_every_name_and_lists_the_first_twenty() {
        // 25 symbols, D01 to D25, and then a library of one other in their
        // place: all 25 are gone. The count says 25; the names stop at 20, in
        // name order, so D01 to D20 are listed and D21 to D25 are not.
        let mut many = String::from(
            r#"{"format": "katana-customisation", "version": 1, "name": "Many", "symbols": ["#,
        );
        for i in 1..=25 {
            if i > 1 {
                many.push_str(", ");
            }
            many.push_str(&format!(
                r#"{{"name": "D{i:02}", "strokes": [["circle", 1]]}}"#
            ));
        }
        many.push_str("]}");
        let one = concat!(
            r#"{"format": "katana-customisation", "version": 1, "name": "One","#,
            r#" "symbols": [{"name": "Z", "strokes": [["circle", 1]]}]}"#
        );
        let scratch = Scratch::new("twenty");
        let mut session = Bare::default();
        session.ok(&format!(
            "CUSTOMISE {}",
            typed(&scratch.write("many.json", &many))
        ));
        assert_eq!(session.document.style_library().size(), 25);

        let reply = session.ok(&format!(
            "CUSTOMISE REPLACE {}",
            typed(&scratch.write("one.json", one))
        ));
        let count = record(&reply, "removed", 0);
        assert_eq!(field(&count, "definitions"), "25");
        assert_eq!(field(&count, "codes"), "0");
        // The record of the count, then one a name.
        assert_eq!(records_of(&reply, "removed"), 21, "{reply}");
        assert_eq!(field(&record(&reply, "removed", 1), "definition"), "D01");
        assert_eq!(field(&record(&reply, "removed", 20), "definition"), "D20");
        assert!(!reply.contains("D21"), "{reply}");
        assert_eq!(session.document.style_library().size(), 1);
    }

    #[test]
    fn a_load_is_all_or_nothing_and_every_file_that_does_not_read_is_named() {
        let mut session = Hosted::new("all-or-nothing");
        session.start();
        let before = session.document.customisation();
        let generation = session.document.customisation_generation();
        let site = session.scratch.write("site.json", SITE);
        let legacy = session.scratch.write("lines.4d", LEGACY_LIBRARY);
        let absent = session.scratch.root.join("nowhere.json");

        // A good file first, then one of another program's: the reader's own
        // words, the file beside them, and nothing of the good file installed.
        let one = session.refused(&format!("CUSTOMISE {} {}", typed(&site), typed(&legacy)));
        assert_eq!(one.code, ErrorCode::ParseFailure);
        assert_eq!(one.message, "not a Katana customisation file");
        assert!(one.context.contains("lines.4d"), "{}", one.describe());

        // Two that do not read: both are listed, each with its reason.
        let two = session.refused(&format!(
            "CUSTOMISE REPLACE {} {} {}",
            typed(&legacy),
            typed(&site),
            typed(&absent)
        ));
        assert!(
            two.message
                .starts_with("2 of the 3 files were not read, so nothing was loaded\n"),
            "{}",
            two.message
        );
        assert!(
            two.message
                .contains("\n  ParseFailure: not a Katana customisation file ["),
            "{}",
            two.message
        );
        assert!(
            two.message
                .contains("\n  NotFound: the file cannot be opened ["),
            "{}",
            two.message
        );
        assert!(two.message.contains("nowhere.json"), "{}", two.message);

        // A survey code file of another program is XML, and is told the same as
        // its style library was.
        let codes = session.scratch.write(
            "codes.mapfile",
            "<?xml version=\"1.0\"?>\n<map_file><map_data/></map_file>\n",
        );
        // It begins with '<', which JSON cannot, and is sent to the converter with
        // none of the parser's account of the character.
        let xml = session.refused(&format!("CUSTOMISE REPLACE {}", typed(&codes)));
        assert_eq!(xml.code, ErrorCode::ParseFailure);
        assert_eq!(
            xml.message,
            concat!(
                "not a Katana customisation file; the older formats are converted with ",
                "katana_customisation_convert (a developer's tool, not installed; see ",
                "docs/customisation.md)"
            )
        );
        // The file, and nothing of the parser's sentence.
        assert!(xml.context.ends_with("codes.mapfile"), "{}", xml.context);
        assert!(!xml.context.contains("syntax"), "{}", xml.context);
        assert!(!xml.context.contains("parse"), "{}", xml.context);

        assert!(session.document.customisation() == before);
        assert_eq!(session.document.customisation_generation(), generation);
        assert_eq!(session.state().origin, CustomisationOrigin::BuiltIn);
    }

    #[test]
    fn a_load_the_session_could_not_be_recorded_with_is_refused_whole() {
        // The merge judges the loads; the install judges the session's own parts.
        // A definition made in the session whose source could never be written
        // into a project (a path, not a name) stops a load that is itself sound.
        let mut session = Hosted::new("install-refused");
        session.start();
        let mut library = session.document.style_library().clone();
        let stray = LineStyle {
            name: String::from("STRAY"),
            source: String::from("somewhere/else"),
            ..LineStyle::default()
        };
        assert!(add_or_replace(&mut library, stray).is_ok());
        session.document.set_style_library(library);
        let before = session.document.customisation();

        let site = session.scratch.write("site.json", SITE);
        let error = session.refused(&format!("CUSTOMISE {}", typed(&site)));
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert!(
            error
                .message
                .contains("the source of its definition \"STRAY\""),
            "{}",
            error.message
        );
        assert!(session.document.customisation() == before);
    }

    // CustomisationVerbs.AKeywordIsTheWholeFirstWordAndAFileSoNamedIsGivenWithItsDirectory
    // changes the working folder: it is in the target tests/customisation_verbs_env.

    #[test]
    fn the_words_that_take_nothing_refuse_a_word_after_them() {
        let mut session = Hosted::new("alone");
        session.start();
        for word in ["JSON", "RESET", "KEEP", "REVERT"] {
            let error = session.refused(&format!("CUSTOMISE {word} now"));
            assert_eq!(error.code, ErrorCode::InvalidArgument, "{word}");
            assert_eq!(error.message, format!("usage: CUSTOMISE {word}"));
        }
        assert!(!session.kept_file.exists());
        assert_eq!(session.state().origin, CustomisationOrigin::BuiltIn);
    }

    // ---- export ----------------------------------------------------------------------------

    #[test]
    fn an_export_read_in_the_sessions_place_gives_the_same_session() {
        // A session with everything in it: the built-in, Site merged in, a
        // control code respelled and a switch turned off.
        let mut first = Hosted::new("export-round-trip");
        first.start();
        let site = first.scratch.write("site.json", SITE);
        first.ok(&format!("CUSTOMISE {}", typed(&site)));
        first.ok("CUSTOMISE SET linework.start=S auto.codes=off");
        let out = first.scratch.root.join("session.customisation.json");

        let exported = record(
            &first.ok(&format!("CUSTOMISE EXPORT {}", typed(&out))),
            "exported",
            0,
        );
        assert_eq!(field(&exported, "file"), shown(&out));
        assert_eq!(field(&exported, "name"), "Small Built In");
        assert_eq!(field(&exported, "definitions"), "3");
        assert_eq!(field(&exported, "codes"), "4");
        assert_eq!(field(&exported, "rules"), "4");
        assert_eq!(field(&exported, "replaced"), "no");
        // An export changes nothing of the session.
        assert_eq!(first.state().origin, CustomisationOrigin::Edited);

        // Read in the place of nothing, in a session with no host at all.
        let mut second = Bare::default();
        second.ok(&format!("CUSTOMISE REPLACE {}", typed(&out)));
        assert!(second.document.customisation() == first.document.customisation());
        assert_eq!(second.state().linework.start, "S");
        assert!(!second.state().automation.codes_on_survey_import);
        assert_eq!(second.state().sources.len(), 2);

        // Written again over the file that is there, and said.
        let again = first.ok(&format!("CUSTOMISE EXPORT {}", typed(&out)));
        assert_eq!(field(&record(&again, "exported", 0), "replaced"), "yes");
        assert!(!beside(&out, ".tmp").exists());
    }

    #[test]
    fn an_export_of_a_part_holds_that_part_and_no_settings() {
        let mut session = Hosted::new("export-parts");
        session.start();
        session.ok("CUSTOMISE SET linework.start=S");

        // The codes alone: both rules, no definition, the colours their names
        // mean - and neither the control codes nor the switches, which merged
        // into a colleague's session would reset theirs.
        let (codes_reply, codes) = exported_to(&mut session, "codes.json", "CODES");
        assert_eq!(field(&codes_reply, "definitions"), "0");
        assert_eq!(field(&codes_reply, "codes"), "2");
        assert_eq!(field(&codes_reply, "rules"), "2");
        assert_eq!(codes.library.size(), 0);
        assert_eq!(codes.map.size(), 2);
        assert_eq!(codes.colours.size(), 1);
        assert!(codes.linework.is_none());
        assert!(codes.automation.is_none());
        assert!(codes.based_on.is_none());
        assert_eq!(codes.name, "Small Built In");
        assert_eq!(codes.notice, ["Written for these tests."]);
        // Of its one source it holds the rules and no definition, and says so.
        assert_eq!(codes.sources, [source("Small Built In", false, true, &[])]);

        // One kind of definition, then the other.
        let (symbols_reply, symbols) = exported_to(&mut session, "symbols.json", "symbols");
        assert_eq!(field(&symbols_reply, "definitions"), "1");
        assert_eq!(field(&symbols_reply, "rules"), "0");
        assert!(symbols.library.contains("TEST Peg"));
        assert_eq!(symbols.library.size(), 1);
        assert_eq!(symbols.map.size(), 0);
        assert_eq!(
            symbols.sources,
            [source("Small Built In", true, false, &[])]
        );
        let (lines_reply, lines) = exported_to(&mut session, "lines.json", "LINESTYLES CODES");
        assert_eq!(field(&lines_reply, "definitions"), "1");
        assert_eq!(field(&lines_reply, "rules"), "2");
        assert!(lines.library.contains("TEST Fence"));
        assert_eq!(lines.sources, [source("Small Built In", true, true, &[])]);

        // Named definitions alone - and no codes unless CODES is said - under a
        // name of the export's own.
        let (only_reply, only) =
            exported_to(&mut session, "peg.json", "ONLY \"TEST Peg\" NAME Pegs");
        assert_eq!(field(&only_reply, "name"), "Pegs");
        assert_eq!(field(&only_reply, "definitions"), "1");
        assert_eq!(field(&only_reply, "codes"), "0");
        assert_eq!(only.name, "Pegs");
        assert_eq!(only.library.size(), 1);
        assert_eq!(only.map.size(), 0);
        let (both_reply, both) = exported_to(
            &mut session,
            "both.json",
            "NAME Both CODES LINESTYLES SYMBOLS ONLY \"TEST Fence\"",
        );
        assert_eq!(field(&both_reply, "definitions"), "1");
        assert_eq!(field(&both_reply, "rules"), "2");
        assert_eq!(both.name, "Both");

        // ONLY chooses among the DEFINITIONS and a kind word among the kinds, so
        // the names are open to both kinds of definition until LINESTYLES or
        // SYMBOLS narrows them. CODES said beside ONLY adds the codes and narrows
        // nothing: it once switched both kinds of definition off, and the symbol
        // asked for by name was refused as "not being written".
        let (codes_and_peg_reply, codes_and_peg) = exported_to(
            &mut session,
            "codes-and-peg.json",
            "CODES ONLY \"TEST Peg\"",
        );
        assert_eq!(field(&codes_and_peg_reply, "definitions"), "1");
        assert_eq!(field(&codes_and_peg_reply, "codes"), "2");
        assert_eq!(field(&codes_and_peg_reply, "rules"), "2");
        assert!(codes_and_peg.library.contains("TEST Peg"));
        assert_eq!(codes_and_peg.library.size(), 1);
        assert_eq!(codes_and_peg.map.size(), 2);
        // A linestyle and a symbol by name, with no kind word at all.
        let (two_reply, two) =
            exported_to(&mut session, "two.json", "ONLY \"TEST Fence\" \"TEST Peg\"");
        assert_eq!(field(&two_reply, "definitions"), "2");
        assert_eq!(field(&two_reply, "rules"), "0");
        assert_eq!(two.library.size(), 2);
        assert_eq!(two.map.size(), 0);

        // The whole session does hold the settings: it is what a KEEP writes.
        let (whole_reply, whole) = exported_to(&mut session, "whole.json", "");
        assert_eq!(field(&whole_reply, "definitions"), "2");
        let linework = whole
            .linework
            .as_ref()
            .expect("the whole session says its linework");
        assert_eq!(linework.start, "S");
        assert!(whole.automation.is_some());
        assert!(whole.based_on.is_some());
        assert_eq!(whole.sources, [source("Small Built In", true, true, &[])]);
    }

    #[test]
    fn a_part_loaded_elsewhere_answers_only_for_what_it_holds() {
        // One symbol of the built-in, exported for a colleague. The session is
        // "Small Built In" - definitions and rules - and the part holds one of
        // its definitions and none of its rules, so that is what the part says:
        // the source, with definitions and without rules.
        let mut author = Hosted::new("part-author");
        author.start();
        let project = author.scratch.root.join("site.katana");
        must(author.document.save_as(&project));
        let pegs = author.scratch.root.join("pegs.json");
        author.ok(&format!(
            "CUSTOMISE EXPORT {} ONLY \"TEST Peg\" NAME Pegs",
            typed(&pegs)
        ));

        let part = parsed(&contents_of(&pegs));
        assert_eq!(part.name, "Pegs");
        assert_eq!(part.sources, [source("Small Built In", true, false, &[])]);
        assert_eq!(part.notice, ["Written for these tests."]);
        assert_eq!(part.library.size(), 1);
        // The definition itself still says which customisation it came from.
        assert_eq!(
            part.library.find("TEST Peg").expect("the peg").source,
            "Small Built In"
        );
        assert!(part.map.empty());

        // The colleague has a customisation of their own, opens the author's
        // project - which was drawn with one they do not have - and loads the
        // part. Their session holds one definition of "Small Built In" and none
        // of its rules, and says exactly that. (The part once listed the
        // author's sources as they stood: "definitions=yes rules=yes", of a
        // customisation the colleague had one symbol of.)
        let mut colleague = Bare::default();
        let own = author.scratch.write("own.json", OWN);
        colleague.ok(&format!("CUSTOMISE {}", typed(&own)));
        must(colleague.document.open(&project));
        assert_eq!(colleague.state().missing_at_open, ["Small Built In"]);
        let loaded = colleague.ok(&format!("CUSTOMISE {}", typed(&pegs)));
        assert_eq!(field(&record(&loaded, "loaded", 0), "name"), "Pegs");
        assert_eq!(
            colleague.state().sources,
            [
                source("Own", true, false, &[]),
                source("Small Built In", true, false, &["Written for these tests."])
            ]
        );
        assert!(colleague.ok("CUSTOMISE").contains(concat!(
            "source name=Own definitions=yes rules=no\n",
            "source name=\"Small Built In\" definitions=yes rules=no\n"
        )));
        assert_eq!(colleague.document.survey_map().size(), 0);

        // What a save records the drawing as drawn with: both, while the
        // colleague's library holds a definition of each.
        must(colleague.document.save());
        assert_eq!(
            colleague.document.metadata().customisation,
            ["Own", "Small Built In"]
        );
        // The one symbol removed again, nothing of "Small Built In" is left in
        // the session, and the next save no longer says the drawing is drawn
        // with it. A source that brought definitions alone is recorded only
        // while it still defines something; one said to have brought RULES is
        // taken at its word for ever, which is what the part's untrue "rules"
        // made of it.
        colleague.ok("CUSTOMISE REMOVE \"TEST Peg\"");
        must(colleague.document.save());
        assert_eq!(colleague.document.metadata().customisation, ["Own"]);
    }

    #[test]
    fn a_part_names_the_sources_of_what_it_holds_and_carries_every_notice() {
        // The built-in with Marks and Tints merged in (the head of
        // verbs_support.rs): sources Small Built In (definitions, rules), Marks
        // (definitions, rules, its notice) and Tints (neither kind, its notice);
        // the session's own notice is the built-in's. Two colours: the
        // built-in's sui gas, which its FE* rule names, and Tints' tint teal,
        // which nothing names until a definition below is given a pen of it.
        //
        // The rule is cad's customisation_part - the one rule the verb and the
        // two managers' export buttons write a part by (customisation_part.rs).
        // SEVEN EXPECTATIONS HERE CHANGED when the verb took it, each marked
        // "was" below. This test first pinned the verb's own rule, by which every
        // colour of the session went with every part, so that a table of colours
        // was a source of every part while the session had a colour at all. By
        // the one rule a part carries only the colours its written rules and
        // pens NAME - the rest would overwrite colours of whoever loads it that
        // nothing in it uses - and so a table of colours is a source only of a
        // part that carries a colour; left out, its notice is written with the
        // part's own, as that of any source left out is. What did NOT change is
        // the verb's side of the rule: a source is listed for what is written of
        // it, and no notice is lost.
        //
        // Nothing else this test pinned was changed or taken out. Its last case,
        // the cross written from a session with no colour at all, stands as it
        // was written: by the verb's own rule that was the one part of which a
        // table of colours was no source, and by the one rule it says what the
        // first case says, the session's colours being beside the point for a
        // symbol with no pen. The two cases of TEST Painted are NEW with the one
        // rule and take no case's place: a pen is all that makes the colours
        // matter to a part of definitions, and no symbol here had one.
        let mut session = Hosted::new("part-sources");
        session.start();
        let marks = session.scratch.write("marks.json", MARKS);
        let tints = session.scratch.write("tints.json", TINTS);
        session.ok(&format!("CUSTOMISE {} {}", typed(&marks), typed(&tints)));
        let marks_notice = ["Marks: drawn by hand."];
        let tints_notice = ["Tints: free to use."];
        assert_eq!(
            session.state().sources,
            [
                source("Small Built In", true, true, &[]),
                source("Marks", true, true, &marks_notice),
                source("Tints", false, false, &tints_notice)
            ]
        );
        assert_eq!(session.state().colours.size(), 2);

        // One symbol, which came from Marks and has no pen. Nothing of the
        // built-in's is written, so it is no source of the part. No colour is
        // named, so none is carried (was: both), Tints is no source (was: one,
        // "for the colours, which go with every part"), and its notice is
        // written after the part's own (was: the part's own alone).
        let cross = part(
            &mut session,
            "cross.json",
            "ONLY \"MARK Cross\" NAME Crosses",
        );
        assert_eq!(cross.sources, [source("Marks", true, false, &marks_notice)]);
        assert_eq!(
            cross.notice,
            ["Written for these tests.", "Tints: free to use."]
        );
        assert!(cross.colours.empty());

        // The codes alone: every source that brought rules, said to have brought
        // rules and no more. FE* names sui gas, so that colour is carried - not
        // tint teal, which no rule names - and with a colour in the part the
        // table of colours is a source of it: nothing says whose a colour is.
        let codes = part(&mut session, "codes.json", "CODES");
        assert_eq!(
            codes.sources,
            [
                source("Small Built In", false, true, &[]),
                source("Marks", false, true, &marks_notice),
                source("Tints", false, false, &tints_notice)
            ]
        );
        assert_eq!(codes.notice, ["Written for these tests."]);
        assert_eq!(codes.map.size(), 3);
        assert_eq!(codes.colours.size(), 1);
        assert_eq!(codes.colours.entries()[0].name, "sui gas");

        // The linestyles alone: TEST Fence, the built-in's, which has no pen.
        // Marks brought a symbol and a rule and neither is written, so it is not
        // a source of this part - and its notice is not dropped for that: it is
        // written with the part's own, after it. Nor is Tints a source, no
        // colour being carried (was: one), and its notice follows (was: absent).
        let lines = part(&mut session, "lines.json", "LINESTYLES");
        assert_eq!(lines.sources, [source("Small Built In", true, false, &[])]);
        assert_eq!(
            lines.notice,
            [
                "Written for these tests.",
                "Marks: drawn by hand.",
                "Tints: free to use."
            ]
        );
        assert!(lines.colours.empty());

        // Loaded into an empty session, the first part brings the one source it
        // lists (was: Tints too) and, of the rest, the notices it carries (was:
        // the built-in's alone).
        let mut other = Bare::default();
        let cross_file = session.scratch.root.join("cross.json");
        other.ok(&format!("CUSTOMISE {}", typed(&cross_file)));
        assert_eq!(other.state().name, "Crosses");
        assert_eq!(
            other.state().notice,
            ["Written for these tests.", "Tints: free to use."]
        );
        assert_eq!(
            other.state().sources,
            [source("Marks", true, false, &marks_notice)]
        );

        // A symbol made in this session, with a pen of the table's colour spelled
        // another way (one name, by the fold a colour name is compared in). The
        // part carries that one colour, as the table spells it, and so the table
        // of colours is a source of it; the symbol itself came from no
        // customisation and adds none.
        let mut library = session.document.style_library().clone();
        let painted = LineStyle {
            name: String::from("TEST Painted"),
            symbol: true,
            at_vertices: true,
            strokes: vec![
                Stroke {
                    op: StrokeOp::Pen,
                    pen: String::from("Tint_Teal"),
                    ..Stroke::default()
                },
                Stroke {
                    op: StrokeOp::Circle,
                    radius: 1.0,
                    ..Stroke::default()
                },
            ],
            ..LineStyle::default()
        };
        assert!(library.add(painted).is_ok());
        session.document.set_style_library(library);
        let teal = part(&mut session, "painted.json", "ONLY \"TEST Painted\"");
        assert_eq!(teal.sources, [source("Tints", false, false, &tints_notice)]);
        assert_eq!(
            teal.notice,
            ["Written for these tests.", "Marks: drawn by hand."]
        );
        assert_eq!(teal.colours.size(), 1);
        assert_eq!(teal.colours.entries()[0].name, "tint teal");

        // With no colour in the session at all, a table of colours is a source
        // of nothing. First the case this test ended with before the one rule,
        // its three expectations UNCHANGED: the cross again, of which Marks is
        // the source and the table of colours none, its notice kept with the
        // part's own. (Worked from the rule as the first case is, and to the
        // same three answers: no pen, so no colour named, whatever the session
        // holds.)
        session.document.set_colour_table(ColourTable::default());
        let bare = part(&mut session, "bare.json", "ONLY \"MARK Cross\"");
        assert_eq!(bare.sources, [source("Marks", true, false, &marks_notice)]);
        assert_eq!(
            bare.notice,
            ["Written for these tests.", "Tints: free to use."]
        );
        assert!(bare.colours.empty());

        // Then the painted symbol again, new as its case above is: its pen names
        // a colour the session no longer has, so none is carried, the table of
        // colours is no source of the part - which then has no source at all -
        // and every source's notice is written with the part's own.
        let bare_painted = part(&mut session, "bare-painted.json", "ONLY \"TEST Painted\"");
        assert!(bare_painted.sources.is_empty());
        assert_eq!(
            bare_painted.notice,
            [
                "Written for these tests.",
                "Marks: drawn by hand.",
                "Tints: free to use."
            ]
        );
        assert!(bare_painted.colours.empty());
    }

    #[test]
    fn the_codes_written_and_read_back_in_their_place_leave_the_sources_with_what_each_brought() {
        // The round trip that edits the codes: EXPORT f CODES, change the file,
        // REPLACE f. The same three-source session as above. The part lists the
        // two sources that brought rules, as bringing rules, and the table of
        // colours; read back in the place of the rules, each source is again
        // what it was - definitions it kept, rules it brought again - and in the
        // same order. (A part that listed no sources would come back as ONE
        // source of the session's name, and Marks would have lost its rules.)
        // The one thing more: a file's own notice goes with the source of its
        // name, so the built-in's entry now says the session's notice too.
        let mut session = Hosted::new("part-round-trip");
        session.start();
        let marks = session.scratch.write("marks.json", MARKS);
        let tints = session.scratch.write("tints.json", TINTS);
        session.ok(&format!("CUSTOMISE {} {}", typed(&marks), typed(&tints)));
        let marks_notice = ["Marks: drawn by hand."];
        let tints_notice = ["Tints: free to use."];
        let file = typed(&session.scratch.root.join("codes.json"));
        session.ok(&format!("CUSTOMISE EXPORT {file} CODES"));
        let reply = session.ok(&format!("CUSTOMISE REPLACE {file}"));
        // The same 3 rules took the place of the 3: nothing is gone.
        assert_eq!(records_of(&reply, "removed"), 0, "{reply}");
        assert_eq!(session.document.survey_map().size(), 3);
        assert_eq!(session.document.style_library().size(), 3);
        assert_eq!(
            session.state().sources,
            [
                source("Small Built In", true, true, &["Written for these tests."]),
                source("Marks", true, true, &marks_notice),
                source("Tints", false, false, &tints_notice)
            ]
        );
        assert_eq!(session.state().name, "Small Built In");
    }

    #[test]
    fn what_an_export_refuses_writes_no_file() {
        let mut session = Hosted::new("export-refused");
        session.start();
        let out = session.scratch.root.join("out.json");
        let file = typed(&out);

        for words in [
            String::new(),
            format!("{file} EVERYTHING"),
            format!("{file} ONLY"),
            format!("{file} NAME"),
            format!("{file} ONLY \"TEST Peg\" NAME A B"),
        ] {
            let error = session.refused(&format!("CUSTOMISE EXPORT {words}"));
            assert_eq!(error.code, ErrorCode::InvalidArgument, "{words}");
            assert!(
                error.message.starts_with("usage: CUSTOMISE EXPORT <file> "),
                "{}",
                error.message
            );
        }
        // A definition the library lacks, and one whose kind is left out: asked
        // for by name, neither is dropped silently.
        assert_eq!(
            session
                .refused(&format!("CUSTOMISE EXPORT {file} ONLY Nowhere"))
                .code,
            ErrorCode::NotFound
        );
        assert_eq!(
            session
                .refused(&format!(
                    "CUSTOMISE EXPORT {file} LINESTYLES ONLY \"TEST Peg\""
                ))
                .code,
            ErrorCode::InvalidArgument
        );
        // After ONLY every word is a definition, so a kind word put there is
        // asked for as one. It is none, and the refusal says where it belongs.
        let after = session.refused(&format!("CUSTOMISE EXPORT {file} ONLY \"TEST Peg\" CODES"));
        assert_eq!(after.code, ErrorCode::NotFound);
        assert_eq!(after.context, "CODES");
        assert_eq!(
            after.message,
            concat!(
                "after ONLY every word is a definition's name, and the library has no definition ",
                "of this name; a word of EXPORT's own goes before ONLY"
            )
        );
        // A name a project could not record.
        let name = session.refused(&format!("CUSTOMISE EXPORT {file} NAME a/b"));
        assert_eq!(name.code, ErrorCode::InvalidArgument);
        assert!(name.message.starts_with("NAME: "), "{}", name.message);
        assert!(!out.exists());
        assert!(!beside(&out, ".tmp").exists());

        // Nothing loaded: nothing to export. Rules set with no customisation
        // ever installed have no name to be written under, until one is given.
        let mut bare = Bare::default();
        assert_eq!(
            bare.refused(&format!("CUSTOMISE EXPORT {file}")).message,
            "no customisation is loaded, so there is nothing to export"
        );
        bare.document.set_survey_map(parsed(SITE).map);
        let unnamed = bare.refused(&format!("CUSTOMISE EXPORT {file}"));
        assert_eq!(unnamed.code, ErrorCode::InvalidState);
        assert!(
            unnamed
                .message
                .contains("CUSTOMISE EXPORT <file> NAME <name>"),
            "{}",
            unnamed.message
        );
        assert!(!out.exists());
        let named = bare.ok(&format!("CUSTOMISE EXPORT {file} NAME Mine"));
        assert_eq!(field(&record(&named, "exported", 0), "rules"), "3");
        assert_eq!(parsed(&contents_of(&out)).name, "Mine");
    }

    // ---- reset, keep, revert ---------------------------------------------------------------

    #[test]
    fn keep_writes_the_session_and_the_next_start_gives_it() {
        let mut session = Hosted::new("keep");
        session.start();
        assert!(session.state().kept);
        session.ok("CUSTOMISE SET auto.codes=off");
        assert!(!session.state().kept);

        let kept = record(&session.ok("CUSTOMISE KEEP"), "kept", 0);
        assert_eq!(field(&kept, "file"), shown(&session.kept_file));
        assert_eq!(field(&kept, "written"), "yes");
        assert_eq!(field(&kept, "name"), "Small Built In");
        assert_eq!(field(&kept, "definitions"), "2");
        assert_eq!(field(&kept, "codes"), "2");
        assert_eq!(field(&kept, "rules"), "2");
        assert_eq!(field(&kept, "backup"), "none");
        assert!(session.state().kept);
        // The folder was made for it, and nothing is left beside it.
        assert!(session.kept_file.exists());
        assert!(!beside(&session.kept_file, ".tmp").exists());
        assert!(!session.backup().exists());

        // What was written is the session, and says what it was made from: the
        // built-in, by name and by the digest of its bytes.
        let written = parsed(&contents_of(&session.kept_file));
        assert!(written == session.document.customisation());
        let based_on = written
            .based_on
            .as_ref()
            .expect("the kept session says what it is based on");
        assert_eq!(based_on.name, "Small Built In");
        assert_eq!(based_on.digest, fnv1a(BUILT_IN));
        let automation = written
            .automation
            .expect("the kept session says its automation");
        assert!(!automation.codes_on_survey_import);

        // A second session, started by the same host: the kept one, not the
        // built-in.
        let mut next = Document::default();
        let start = start_customisation(&mut next, &session.host);
        assert_eq!(start.installed, CustomisationOrigin::Kept);
        assert!(!start.kept_from_another_built_in);
        assert!(!next.customisation_state().automation.codes_on_survey_import);
    }

    #[test]
    fn a_later_keep_leaves_the_earlier_file_beside_it_and_revert_reads_the_kept_one() {
        let mut session = Hosted::new("keep-twice");
        session.start();
        session.ok("CUSTOMISE SET auto.codes=off");
        session.ok("CUSTOMISE KEEP");
        let first = contents_of(&session.kept_file);

        session.ok("CUSTOMISE SET linework.start=S");
        let kept = record(&session.ok("CUSTOMISE KEEP"), "kept", 0);
        assert_eq!(field(&kept, "written"), "yes");
        assert_eq!(field(&kept, "backup"), shown(&session.backup()));
        assert_eq!(contents_of(&session.backup()), first);
        assert_ne!(contents_of(&session.kept_file), first);

        // An edit that is not kept, then back to what is.
        session.ok("CUSTOMISE SET auto.linework=off linework.start=BEGIN");
        assert!(!session.state().kept);
        let reverted = record(&session.ok("CUSTOMISE REVERT"), "reverted", 0);
        assert_eq!(field(&reverted, "file"), shown(&session.kept_file));
        assert_eq!(field(&reverted, "name"), "Small Built In");
        assert_eq!(field(&reverted, "rules"), "2");
        assert_eq!(session.state().origin, CustomisationOrigin::Kept);
        assert!(session.state().kept);
        assert_eq!(session.state().linework.start, "S");
        assert!(session.state().automation.linework_on_survey_import);
        assert!(!session.state().automation.codes_on_survey_import);

        // A third keep: the .bak is the file as it was before THIS keep, and the
        // .bak that was there is replaced by it.
        let second = contents_of(&session.kept_file);
        session.ok("CUSTOMISE SET auto.linework=off");
        assert_eq!(
            field(&record(&session.ok("CUSTOMISE KEEP"), "kept", 0), "backup"),
            shown(&session.backup())
        );
        assert_eq!(contents_of(&session.backup()), second);
        assert_ne!(contents_of(&session.kept_file), second);
        assert!(!beside(&session.kept_file, ".tmp").exists());
    }

    #[test]
    fn reset_gives_the_built_in_and_keeping_it_retires_the_kept_file() {
        let mut session = Hosted::new("reset");
        session.start();
        let site = session.scratch.write("site.json", SITE);
        session.ok(&format!("CUSTOMISE {}", typed(&site)));
        session.ok("CUSTOMISE SET linework.start=S auto.codes=off");
        session.ok("CUSTOMISE KEEP");
        let kept_text = contents_of(&session.kept_file);

        // The built-in, whole: Site's definitions and rules are gone, and the
        // control code and the switch are the built-in's again - which says
        // neither, so the defaults. Not "kept": a kept file is there, and the
        // next start would read that.
        assert_eq!(
            session.ok("CUSTOMISE RESET"),
            "reset name=\"Small Built In\" definitions=2 codes=2 rules=2 kept=no"
        );
        assert_eq!(session.state().origin, CustomisationOrigin::BuiltIn);
        assert!(!session.state().kept);
        assert_eq!(session.state().linework.start, "ST");
        assert!(session.state().automation.codes_on_survey_import);
        assert_eq!(session.document.survey_map().size(), 2);
        let based_on = session
            .state()
            .based_on
            .clone()
            .expect("the built-in is based on itself");
        assert_eq!(based_on.digest, fnv1a(BUILT_IN));
        assert_eq!(
            contents_of(&session.kept_file),
            kept_text,
            "a RESET writes nothing"
        );

        // Kept as it is, a copy of the built-in would be read at every later
        // start in the place of whatever the built-in had become. So nothing is
        // written, and the kept file is set aside.
        let kept = record(&session.ok("CUSTOMISE KEEP"), "kept", 0);
        assert_eq!(field(&kept, "file"), shown(&session.kept_file));
        assert_eq!(field(&kept, "written"), "no");
        assert_eq!(field(&kept, "reason"), "the-session-is-the-built-in");
        assert_eq!(field(&kept, "backup"), shown(&session.backup()));
        assert!(!session.kept_file.exists());
        assert_eq!(contents_of(&session.backup()), kept_text);
        assert!(session.state().kept);

        // The next start gives the built-in, and a KEEP there has nothing to do.
        let mut next = Document::default();
        assert_eq!(
            start_customisation(&mut next, &session.host).installed,
            CustomisationOrigin::BuiltIn
        );
        let again = record(&session.ok("CUSTOMISE KEEP"), "kept", 0);
        assert_eq!(field(&again, "written"), "no");
        assert_eq!(field(&again, "backup"), "none");
        assert!(!session.kept_file.exists());
        // With no kept file, a RESET is what the next start gives.
        assert_eq!(
            session.ok("CUSTOMISE RESET"),
            "reset name=\"Small Built In\" definitions=2 codes=2 rules=2 kept=yes"
        );
    }

    // ---- the built-ins ---------------------------------------------------------------------
    //
    // CUSTOMISE BUILTIN [<name>]: a host that offers two built-ins, BUILT_IN (the
    // one it starts with, so the default) and SITE (two_built_ins). The counts
    // are those at the top of verbs_support.rs.

    #[test]
    fn built_in_alone_lists_the_built_ins_with_the_default_and_the_active_one_marked() {
        let mut session = two_built_ins("builtin-list");
        session
            .host
            .built_ins
            .as_mut()
            .expect("the host offers its own list")[0]
            .source = String::from("small.customisation.json");
        session.start();
        let reply = session.ok("CUSTOMISE BUILTIN");

        let header = record(&reply, "builtins", 0);
        assert_eq!(field(&header, "count"), "2");
        assert_eq!(field(&header, "default"), "Small Built In");
        assert_eq!(field(&header, "active"), "Small Built In");
        assert_eq!(
            field(&header, "based_on"),
            "(absent)",
            "the session IS a built-in"
        );
        assert_eq!(records_of(&reply, "builtin"), 2);
        let first = record(&reply, "builtin", 0);
        assert_eq!(field(&first, "name"), "Small Built In");
        assert_eq!(field(&first, "default"), "yes");
        assert_eq!(field(&first, "active"), "yes");
        assert_eq!(field(&first, "definitions"), "2");
        assert_eq!(field(&first, "codes"), "2");
        assert_eq!(field(&first, "rules"), "2");
        assert_eq!(field(&first, "source"), "small.customisation.json");
        assert_eq!(
            field(&first, "description"),
            "Two definitions and two rules, for tests"
        );
        let second = record(&reply, "builtin", 1);
        assert_eq!(field(&second, "name"), "Site");
        assert_eq!(field(&second, "default"), "no");
        assert_eq!(field(&second, "active"), "no");
        assert_eq!(field(&second, "definitions"), "2");
        assert_eq!(field(&second, "codes"), "3");
        assert_eq!(field(&second, "rules"), "3");
        assert_eq!(field(&second, "source"), "site.customisation.json");
        assert_eq!(field(&second, "description"), "");
        assert!(!reply.ends_with('\n'));
        // A list changes nothing.
        assert_eq!(session.state().name, "Small Built In");
        assert!(session.state().kept);
    }

    #[test]
    fn built_in_with_a_name_makes_that_built_in_the_session_and_reset_still_gives_the_default() {
        let mut session = two_built_ins("builtin-use");
        session.start();
        let used = record(&session.ok("CUSTOMISE BUILTIN Site"), "used", 0);
        assert_eq!(field(&used, "name"), "Site");
        assert_eq!(field(&used, "default"), "no");
        assert_eq!(field(&used, "definitions"), "2");
        assert_eq!(field(&used, "codes"), "3");
        assert_eq!(field(&used, "rules"), "3");
        // Not what the next start gives: that is the default, until a KEEP.
        assert_eq!(field(&used, "kept"), "no");

        // The session is the built-in, as RESET leaves its own: the origin, and
        // based on ITSELF - its name, the digest of its bytes.
        assert_eq!(session.state().origin, CustomisationOrigin::BuiltIn);
        assert_eq!(session.state().name, "Site");
        assert!(!session.state().kept);
        let based_on = session
            .state()
            .based_on
            .clone()
            .expect("a built-in is based on itself");
        assert_eq!(based_on.name, "Site");
        assert_eq!(based_on.digest, fnv1a(SITE));
        assert_eq!(session.document.style_library().size(), 2);
        assert_eq!(session.document.survey_map().size(), 3);
        // What the project's earlier names are answered with is the RUN's
        // built-in, not the one chosen.
        assert_eq!(session.state().built_in, "Small Built In");

        // The list follows: Site is the one the session is, Small still the default.
        let header = record(&session.ok("CUSTOMISE BUILTIN"), "builtins", 0);
        assert_eq!(field(&header, "default"), "Small Built In");
        assert_eq!(field(&header, "active"), "Site");

        // RESET is the built-in OF THE RUN, the default here. No kept file is
        // there, so it is what the next start gives.
        assert_eq!(
            session.ok("CUSTOMISE RESET"),
            "reset name=\"Small Built In\" definitions=2 codes=2 rules=2 kept=yes"
        );
        assert_eq!(session.state().name, "Small Built In");
        assert_eq!(
            field(
                &record(&session.ok("CUSTOMISE BUILTIN"), "builtins", 0),
                "active"
            ),
            "Small Built In"
        );

        // The default chosen by name is the same as a RESET, and says it is the default.
        session.ok("CUSTOMISE BUILTIN Site");
        assert_eq!(
            session.ok("CUSTOMISE BUILTIN Small Built In"),
            "used name=\"Small Built In\" default=yes definitions=2 codes=2 rules=2 kept=yes"
        );
    }

    #[test]
    fn built_in_takes_the_name_in_any_letter_case_quoted_or_not_and_kept_stays_no_with_a_kept_file()
    {
        let mut session = two_built_ins("builtin-spelling");
        session.start();
        assert_eq!(
            field(
                &record(&session.ok("customise builtin SITE"), "used", 0),
                "name"
            ),
            "Site"
        );
        assert_eq!(
            field(
                &record(
                    &session.ok("CUSTOMISE BUILTIN \"small built in\""),
                    "used",
                    0
                ),
                "name"
            ),
            "Small Built In"
        );
        // Three words by the time a verb sees an unquoted name: joined by a blank.
        session.ok("CUSTOMISE BUILTIN Site");
        assert_eq!(
            field(
                &record(&session.ok("CUSTOMISE BUILTIN small built in"), "used", 0),
                "name"
            ),
            "Small Built In"
        );

        // With a kept file there, even the run's own is not what the next start
        // gives: as RESET.
        session.ok("CUSTOMISE SET auto.codes=off");
        session.ok("CUSTOMISE KEEP");
        assert_eq!(
            session.ok("CUSTOMISE BUILTIN Small Built In"),
            "used name=\"Small Built In\" default=yes definitions=2 codes=2 rules=2 kept=no"
        );
    }

    #[test]
    fn built_in_of_a_name_the_program_lacks_is_refused_listing_the_names_and_changes_nothing() {
        let mut session = two_built_ins("builtin-unknown");
        session.start();
        session.ok("CUSTOMISE SET auto.codes=off");
        let generation = session.document.customisation_generation();
        let error = session.refused("CUSTOMISE BUILTIN Nowhere");
        assert_eq!(error.code, ErrorCode::NotFound);
        assert!(
            error
                .message
                .contains("no built-in customisation of that name"),
            "{}",
            error.message
        );
        assert!(
            error.message.contains("\"Small Built In\", \"Site\""),
            "{}",
            error.message
        );
        assert_eq!(error.context, "Nowhere");
        assert_eq!(session.document.customisation_generation(), generation);
        assert!(
            !session.state().automation.codes_on_survey_import,
            "the session is as it was"
        );

        // A host that offers none says so.
        let mut none = Hosted::with("builtin-none", false, true);
        none.start();
        assert!(
            none.refused("CUSTOMISE BUILTIN Site")
                .message
                .contains("it has none")
        );
        assert_eq!(
            field(
                &record(&none.ok("CUSTOMISE BUILTIN"), "builtins", 0),
                "count"
            ),
            "0"
        );
    }

    #[test]
    fn built_in_is_refused_with_no_host_and_for_a_built_in_that_did_not_read() {
        let mut bare = Bare::default();
        let needs = bare.refused("CUSTOMISE BUILTIN");
        assert_eq!(needs.code, ErrorCode::InvalidState);
        assert!(
            needs
                .message
                .contains("CUSTOMISE BUILTIN needs the program's built-in"),
            "{}",
            needs.message
        );
        assert_eq!(
            bare.refused("CUSTOMISE BUILTIN Site").code,
            ErrorCode::InvalidState
        );

        // A file compiled in that is no customisation is listed under its file
        // name with the reason, and cannot be used.
        let mut session = two_built_ins("builtin-damaged");
        let damaged = BuiltInEntry {
            name: String::from("damaged.customisation.json"),
            source: String::from("damaged.customisation.json"),
            built_in: BuiltInCustomisation {
                problem: String::from(
                    "the customisation compiled into this program is not read: damaged",
                ),
                ..BuiltInCustomisation::default()
            },
            ..BuiltInEntry::default()
        };
        session
            .host
            .built_ins
            .as_mut()
            .expect("the host offers its own list")
            .push(damaged.clone());
        session.start();
        let reply = session.ok("CUSTOMISE BUILTIN");
        assert_eq!(field(&record(&reply, "builtins", 0), "count"), "3");
        let third = record(&reply, "builtin", 2);
        assert_eq!(field(&third, "name"), "damaged.customisation.json");
        assert_eq!(field(&third, "definitions"), "0");
        assert_eq!(field(&third, "problem"), damaged.built_in.problem);
        let refused = session.refused("CUSTOMISE BUILTIN damaged.customisation.json");
        assert_eq!(refused.code, ErrorCode::InvalidState);
        assert!(
            refused
                .message
                .contains("cannot use the built-in customisation"),
            "{}",
            refused.message
        );
        assert_eq!(session.state().name, "Small Built In");
    }

    // The rule for a KEEP: it retires the kept file ONLY when the session is the
    // built-in of the run. Any other built-in is kept like any session, and the
    // file says which it is based on, so that the next start comes up as it.
    #[test]
    fn an_alternate_built_in_chosen_and_kept_is_what_the_next_start_gives_and_nothing_is_warned_of()
    {
        let mut session = two_built_ins("builtin-keep");
        session.start();
        session.ok("CUSTOMISE BUILTIN Site");
        assert!(!session.state().kept);

        let kept = record(&session.ok("CUSTOMISE KEEP"), "kept", 0);
        assert_eq!(
            field(&kept, "written"),
            "yes",
            "not the built-in OF THE RUN: written like any"
        );
        assert_eq!(field(&kept, "name"), "Site");
        assert_eq!(field(&kept, "definitions"), "2");
        assert_eq!(field(&kept, "codes"), "3");
        assert_eq!(field(&kept, "rules"), "3");
        assert_eq!(field(&kept, "backup"), "none");
        assert!(session.state().kept);
        let written = parsed(&contents_of(&session.kept_file));
        assert_eq!(written.name, "Site");
        let based_on = written
            .based_on
            .as_ref()
            .expect("the kept session says what it is based on");
        assert_eq!(based_on.name, "Site");
        assert_eq!(based_on.digest, fnv1a(SITE));

        // The next start: the kept copy, from a built-in this program has in the
        // edition it has - so it is not "another built-in".
        let start = start_customisation(&mut session.document, &session.host);
        assert_eq!(start.installed, CustomisationOrigin::Kept);
        assert_eq!(start.name, "Site");
        assert!(!start.kept_from_another_built_in);
        assert!(start.problems.is_empty());
        assert_eq!(
            session.state().built_in,
            "Small Built In",
            "the run's built-in is still the default"
        );
        assert!(!session.state().kept_from_another_built_in);

        // The list says what the session is NOT, and what it was made from.
        let header = record(&session.ok("CUSTOMISE BUILTIN"), "builtins", 0);
        assert_eq!(field(&header, "default"), "Small Built In");
        assert_eq!(field(&header, "active"), "");
        assert_eq!(field(&header, "based_on"), "Site");
        assert_eq!(
            field(
                &record(&session.ok("CUSTOMISE BUILTIN"), "builtin", 1),
                "active"
            ),
            "no"
        );
    }

    #[test]
    fn a_reset_and_a_keep_after_a_kept_alternate_retire_the_file_and_the_default_comes_back() {
        let mut session = two_built_ins("builtin-keep-reset");
        session.start();
        session.ok("CUSTOMISE BUILTIN Site");
        session.ok("CUSTOMISE KEEP");
        let kept_text = contents_of(&session.kept_file);

        // A kept file is there, so the reset is not what the next start gives.
        assert_eq!(
            session.ok("CUSTOMISE RESET"),
            "reset name=\"Small Built In\" definitions=2 codes=2 rules=2 kept=no"
        );
        let retired = record(&session.ok("CUSTOMISE KEEP"), "kept", 0);
        assert_eq!(field(&retired, "written"), "no");
        assert_eq!(field(&retired, "reason"), "the-session-is-the-built-in");
        assert_eq!(field(&retired, "backup"), shown(&session.backup()));
        assert!(!session.kept_file.exists());
        assert_eq!(contents_of(&session.backup()), kept_text);

        let start = start_customisation(&mut session.document, &session.host);
        assert_eq!(start.installed, CustomisationOrigin::BuiltIn);
        assert_eq!(start.name, "Small Built In");
    }

    #[test]
    fn choosing_the_default_by_name_after_a_kept_alternate_retires_the_file_like_a_reset() {
        let mut session = two_built_ins("builtin-keep-default");
        session.start();
        session.ok("CUSTOMISE BUILTIN Site");
        session.ok("CUSTOMISE KEEP");
        assert_eq!(
            session.ok("CUSTOMISE BUILTIN Small Built In"),
            "used name=\"Small Built In\" default=yes definitions=2 codes=2 rules=2 kept=no"
        );
        assert_eq!(
            field(&record(&session.ok("CUSTOMISE KEEP"), "kept", 0), "written"),
            "no"
        );
        assert!(!session.kept_file.exists());
        assert!(session.backup().exists());
    }

    // What Settings' Use warns of before it is pressed (it is followed by KEEP on a
    // kept session): ONE earlier file is kept, so a kept customisation of the
    // person's own is put away by the alternate that is kept over it, and put away
    // again - gone - by the default that is kept after that.
    #[test]
    fn an_alternate_then_the_default_kept_in_turn_leave_the_persons_own_kept_file_in_neither_file()
    {
        let mut session = two_built_ins("builtin-keep-twice");
        session.start();
        session.ok("CUSTOMISE SET auto.codes=off");
        assert_eq!(
            field(&record(&session.ok("CUSTOMISE KEEP"), "kept", 0), "written"),
            "yes"
        );
        let personal = contents_of(&session.kept_file);
        let own = parsed(&personal);
        let automation = own
            .automation
            .expect("the person's own session says its automation");
        assert!(!automation.codes_on_survey_import);

        // The alternate over it: written, and the personal file beside it.
        session.ok("CUSTOMISE BUILTIN Site");
        let over = record(&session.ok("CUSTOMISE KEEP"), "kept", 0);
        assert_eq!(field(&over, "written"), "yes");
        assert_eq!(field(&over, "backup"), shown(&session.backup()));
        assert_eq!(contents_of(&session.backup()), personal);
        let alternate = contents_of(&session.kept_file);
        assert_ne!(alternate, personal);

        // The default after it: nothing is written, the alternate's file is set
        // aside in the place of the personal one, which is in neither file.
        session.ok("CUSTOMISE BUILTIN Small Built In");
        let retired = record(&session.ok("CUSTOMISE KEEP"), "kept", 0);
        assert_eq!(field(&retired, "written"), "no");
        assert!(!session.kept_file.exists());
        assert_eq!(contents_of(&session.backup()), alternate);
        assert_ne!(contents_of(&session.backup()), personal);
    }

    #[test]
    fn an_alternate_edited_after_being_chosen_is_kept_as_an_edited_copy_based_on_it() {
        let mut session = two_built_ins("builtin-keep-edited");
        session.start();
        session.ok("CUSTOMISE BUILTIN Site");
        session.ok("CUSTOMISE SET auto.codes=off");
        assert_eq!(session.state().origin, CustomisationOrigin::Edited);
        assert_eq!(
            field(&record(&session.ok("CUSTOMISE KEEP"), "kept", 0), "written"),
            "yes"
        );

        // Not any built-in now, but made from Site.
        let reply = session.ok("CUSTOMISE BUILTIN");
        assert_eq!(field(&record(&reply, "builtins", 0), "active"), "");
        assert_eq!(field(&record(&reply, "builtins", 0), "based_on"), "Site");
        let start = start_customisation(&mut session.document, &session.host);
        assert_eq!(start.installed, CustomisationOrigin::Kept);
        assert!(!start.kept_from_another_built_in);
        assert!(!session.state().automation.codes_on_survey_import);
    }

    #[test]
    fn json_lists_the_built_ins_the_host_offers_with_the_one_the_session_is_active() {
        let mut session = two_built_ins("builtin-json");
        session.start();
        session.ok("CUSTOMISE BUILTIN Site");
        let json = session.ok("CUSTOMISE JSON");
        // Two entries in the host's order, alphabetical keys: Small is the
        // default, Site is the one the session is. Counts by hand, as above.
        assert!(
            json.contains(
                r#"  "builtIns": [
    {
      "active": false,
      "codes": 2,
      "default": true,
      "definitions": 2,
      "description": "Two definitions and two rules, for tests",
      "name": "Small Built In",
      "rules": 2,
      "source": ""
    },
    {
      "active": true,
      "codes": 3,
      "default": false,
      "definitions": 2,
      "description": "",
      "name": "Site",
      "rules": 3,
      "source": "site.customisation.json"
    }
  ],"#
            ),
            "{json}"
        );
    }

    // A project saved while an alternate built-in was the session records it by
    // name. Opened where another is the session, that name is "not loaded" - and it
    // is no file, so CUSTOMISE <file> cannot load it; the verb that does is the one
    // the front ends' warning names (customisation_host's built_in_advice_for).
    #[test]
    fn a_built_in_named_as_not_loaded_by_a_project_is_loaded_by_customise_built_in() {
        let mut author = two_built_ins("builtin-missing-author");
        author.start();
        author.ok("CUSTOMISE BUILTIN Site");
        let project = author.scratch.root.join("drawn.katana");
        must(author.document.save_as(&project));

        let mut session = two_built_ins("builtin-missing-session");
        session.start();
        must(session.document.open(&project));
        assert_eq!(
            session.document.customisation_state().missing_at_open,
            ["Site"]
        );
        // What a front end tells a person about it, from the very list the verb lists.
        assert_eq!(
            built_in_advice_for(
                &session.document.customisation_state().missing_at_open,
                built_ins_of(&session.host)
            ),
            "CUSTOMISE BUILTIN Site makes the built-in \"Site\" the session"
        );

        session.ok("CUSTOMISE BUILTIN Site");
        assert!(
            session
                .document
                .customisation_state()
                .missing_at_open
                .is_empty(),
            "the built-in the project was drawn with is the session now"
        );
        assert!(
            session
                .ok("CUSTOMISE JSON")
                .contains("  \"missing\": [],\n")
        );
    }

    #[test]
    fn keep_is_refused_when_the_kept_file_changed_on_disk_since_the_session_read_it() {
        let mut session = Hosted::new("keep-changed");
        session.start();
        session.ok("CUSTOMISE SET auto.codes=off");
        session.ok("CUSTOMISE KEEP");

        // Another Katana keeps its own there.
        let others = concat!(
            r#"{"format": "katana-customisation", "version": 1, "name": "Theirs","#,
            r#" "codes": [{"key": "ZZ*", "sets": "feature", "layer": "THEIRS"}]}"#
        );
        write_file(&session.kept_file, others);

        session.ok("CUSTOMISE SET linework.start=S");
        let error = session.refused("CUSTOMISE KEEP");
        assert_eq!(error.code, ErrorCode::InvalidState);
        assert!(
            error
                .message
                .contains("changed on disk since this session read it"),
            "{}",
            error.message
        );
        assert!(
            error.message.contains("CUSTOMISE REVERT"),
            "{}",
            error.message
        );
        assert_eq!(
            contents_of(&session.kept_file),
            others,
            "what the other kept is not written over"
        );
        assert!(!session.backup().exists());
        assert!(!session.state().kept);

        // REVERT reads it; from then on it is the file this session saw, and a
        // KEEP writes over it, leaving it beside as the .bak.
        session.ok("CUSTOMISE REVERT");
        assert_eq!(session.state().name, "Theirs");
        session.ok("CUSTOMISE SET auto.linework=off");
        assert_eq!(
            field(&record(&session.ok("CUSTOMISE KEEP"), "kept", 0), "written"),
            "yes"
        );
        assert_eq!(contents_of(&session.backup()), others);
    }

    #[test]
    fn keep_is_refused_over_a_file_that_appeared_and_over_one_that_cannot_be_read_at_all() {
        // It was not there when the session began, and is now.
        {
            let mut session = Hosted::new("keep-appeared");
            session.start();
            session.ok("CUSTOMISE SET auto.codes=off");
            make_folder_of(&session.kept_file);
            write_file(&session.kept_file, SITE);
            let error = session.refused("CUSTOMISE KEEP");
            assert!(
                error.message.contains("changed on disk"),
                "{}",
                error.message
            );
            assert_eq!(contents_of(&session.kept_file), SITE);
        }
        // Something is there that cannot be read at all - here a folder of the
        // kept file's own name. It is not written over unseen either.
        {
            let mut session = Hosted::new("keep-cannot-read");
            fs::create_dir_all(&session.kept_file).expect("the folder is made");
            session.start();
            session.ok("CUSTOMISE SET auto.codes=off");
            let error = session.refused("CUSTOMISE KEEP");
            assert_eq!(error.code, ErrorCode::InvalidState);
            assert_eq!(
                error.message,
                concat!(
                    "CUSTOMISE KEEP: the kept customisation file cannot be read, so it is not ",
                    "written over: the file cannot be opened: it is a directory"
                )
            );
            assert!(session.kept_file.is_dir());
            assert!(!session.state().kept);
        }
    }

    // A kept file that was read at the start and is not a customisation - empty,
    // damaged, or written by a newer Katana - made every later KEEP refuse, and the
    // window had no way to move it: an edit in Settings kept nothing, for good,
    // while the record said `kept=yes`. It is set aside whole as `.bad` and the
    // session is written.
    #[test]
    fn keep_sets_a_kept_file_that_does_not_read_aside_as_bad_and_writes_the_session() {
        let cases: [(&str, &str); 3] = [
            ("damaged", "{ not json"),
            ("empty", ""),
            (
                "newer",
                r#"{"format": "katana-customisation", "version": 2, "name": "Later"}"#,
            ),
        ];
        for (name, bytes) in cases {
            let mut session = Hosted::new(&format!("keep-bad-{name}"));
            make_folder_of(&session.kept_file);
            write_file(&session.kept_file, bytes);
            session.start();
            assert_eq!(
                session.state().origin,
                CustomisationOrigin::BuiltIn,
                "the built-in stands in for a kept file that does not read: {name}"
            );
            assert!(!session.state().start_problems.is_empty(), "{name}");
            if name == "newer" {
                // REVERT still says why it cannot read it.
                assert_eq!(
                    session.refused("CUSTOMISE REVERT").code,
                    ErrorCode::Unsupported
                );
            }
            session.ok("CUSTOMISE SET auto.codes=off");

            let kept = record(&session.ok("CUSTOMISE KEEP"), "kept", 0);
            assert_eq!(field(&kept, "file"), shown(&session.kept_file), "{name}");
            assert_eq!(field(&kept, "written"), "yes", "{name}");
            assert_eq!(field(&kept, "backup"), "none", "{name}");
            assert_eq!(field(&kept, "set_aside"), shown(&session.bad()), "{name}");
            // The file as it was, whole, beside; no backup, which is of a customisation.
            assert_eq!(contents_of(&session.bad()), bytes, "{name}");
            assert!(!session.backup().exists(), "{name}");
            // What is there now is the session, and the folder holds nothing else.
            assert!(
                parsed(&contents_of(&session.kept_file)) == session.document.customisation(),
                "{name}"
            );
            assert!(session.state().kept, "{name}");
            assert_eq!(
                session.kept_folder(),
                ["customisation.json", "customisation.json.bad"],
                "{name}"
            );

            // The next start is good: it reads the kept file, and says nothing is wrong.
            let mut next = Document::default();
            let start = start_customisation(&mut next, &session.host);
            assert_eq!(start.installed, CustomisationOrigin::Kept, "{name}");
            assert!(start.problems.is_empty(), "{name}");
            assert!(
                !next.customisation_state().automation.codes_on_survey_import,
                "{name}"
            );
        }
    }

    #[test]
    fn a_file_set_aside_as_bad_replaces_an_earlier_one_of_that_name() {
        let mut session = Hosted::new("keep-bad-twice");
        make_folder_of(&session.kept_file);
        write_file(&session.kept_file, "the newer damage");
        write_file(&session.bad(), "the earlier damage");
        session.start();
        session.ok("CUSTOMISE SET auto.codes=off");
        session.ok("CUSTOMISE KEEP");
        assert_eq!(contents_of(&session.bad()), "the newer damage");
    }

    #[test]
    fn keeping_the_built_in_over_a_kept_file_that_does_not_read_sets_it_aside_as_bad_too() {
        // The session IS the built-in (the start fell back to it), so nothing is
        // written; the kept file that did not read goes aside, and the next start
        // gives the built-in and has no problem to report.
        let mut session = Hosted::new("retire-bad");
        make_folder_of(&session.kept_file);
        write_file(&session.kept_file, "{ not json");
        session.start();
        assert_eq!(session.state().origin, CustomisationOrigin::BuiltIn);
        let kept = record(&session.ok("CUSTOMISE KEEP"), "kept", 0);
        assert_eq!(field(&kept, "written"), "no");
        assert_eq!(field(&kept, "reason"), "the-session-is-the-built-in");
        assert_eq!(field(&kept, "backup"), "none");
        assert_eq!(field(&kept, "set_aside"), shown(&session.bad()));
        assert!(!session.kept_file.exists());
        assert_eq!(contents_of(&session.bad()), "{ not json");
        let mut next = Document::default();
        let start = start_customisation(&mut next, &session.host);
        assert_eq!(start.installed, CustomisationOrigin::BuiltIn);
        assert!(start.problems.is_empty());
    }

    // The backup is written from the bytes KEEP read and judged, not copied from
    // the file, which carries the file's attributes: a read-only kept file made a
    // read-only `.bak`, and the next KEEP could not replace it - refused, for good,
    // with "could not be kept beside it".
    #[test]
    fn the_backup_is_written_from_what_was_read_so_a_read_only_kept_file_leaves_no_read_only_backup()
     {
        let mut session = Hosted::new("keep-read-only");
        session.start();
        session.ok("CUSTOMISE SET auto.codes=off");
        session.ok("CUSTOMISE KEEP");
        let first = contents_of(&session.kept_file);

        make_read_only(&session.kept_file);
        session.ok("CUSTOMISE SET linework.start=S");
        // Where a read-only file cannot be replaced the KEEP is refused and the
        // kept file stands; where it can, it is taken. Neither is the point.
        let _attempt = session
            .interpreter
            .run(&mut session.document, "CUSTOMISE KEEP");
        if session.backup().exists() {
            assert_eq!(contents_of(&session.backup()), first);
            assert!(
                owner_may_write(&session.backup()),
                "a backup written fresh is not read-only because the kept file was"
            );
        }
        let_owner_write(&session.kept_file);
        session.ok("CUSTOMISE SET linework.end=E");
        let again = record(&session.ok("CUSTOMISE KEEP"), "kept", 0);
        assert_eq!(field(&again, "written"), "yes");
        assert!(session.state().kept);
    }

    // The temporary a write goes through is named for this process and is never
    // `<file>.tmp`, which is a name a person may have used and a write must leave
    // as it found it. Nothing is left beside.
    #[test]
    fn a_write_never_consumes_a_file_named_like_its_old_temporary_and_leaves_none_beside() {
        let mut session = Hosted::new("tmp-name");
        session.start();
        session.ok("CUSTOMISE SET auto.codes=off");
        make_folder_of(&session.kept_file);
        let mine = beside(&session.kept_file, ".tmp");
        write_file(&mine, "my own file");
        session.ok("CUSTOMISE KEEP");
        assert_eq!(contents_of(&mine), "my own file");
        session.ok("CUSTOMISE SET linework.start=S");
        session.ok("CUSTOMISE KEEP");
        assert_eq!(contents_of(&mine), "my own file");
        assert_eq!(
            session.kept_folder(),
            [
                "customisation.json",
                "customisation.json.bak",
                "customisation.json.tmp"
            ]
        );

        // The same for an export, to a name with such a neighbour.
        let out = session.scratch.root.join("out.customisation.json");
        let neighbour = beside(&out, ".tmp");
        write_file(&neighbour, "also mine");
        session.ok(&format!("CUSTOMISE EXPORT {}", typed(&out)));
        assert_eq!(contents_of(&neighbour), "also mine");
        assert!(out.exists());
    }

    // Retiring the kept file (the session IS the built-in) moves it over the
    // earlier backup, which the rename replaces: the earlier backup is not removed
    // first, so a rename that then fails leaves both files where they were.
    #[test]
    fn retiring_the_kept_file_replaces_an_earlier_backup_and_a_lost_race_leaves_both_files_be() {
        {
            let mut session = Hosted::new("retire-bak");
            session.start();
            session.ok("CUSTOMISE SET auto.codes=off");
            session.ok("CUSTOMISE KEEP");
            let kept = contents_of(&session.kept_file);
            session.ok("CUSTOMISE RESET");
            write_file(&session.backup(), "an earlier backup");
            let reply = record(&session.ok("CUSTOMISE KEEP"), "kept", 0);
            assert_eq!(field(&reply, "written"), "no");
            assert_eq!(contents_of(&session.backup()), kept);
            assert!(!session.kept_file.exists());
        }
        // The backup's place taken by a folder with something in it: the kept file
        // cannot be moved there. It is refused, and the kept file, the folder and
        // what is in it are as they were.
        {
            let mut session = Hosted::new("retire-blocked");
            session.start();
            session.ok("CUSTOMISE SET auto.codes=off");
            session.ok("CUSTOMISE KEEP");
            let kept = contents_of(&session.kept_file);
            session.ok("CUSTOMISE RESET");
            fs::create_dir_all(session.backup()).expect("the folder is made");
            write_file(&session.backup().join("inside"), "still here");
            let error = session.refused("CUSTOMISE KEEP");
            assert_eq!(error.code, ErrorCode::FileExportFailure);
            assert_eq!(
                error.message,
                "CUSTOMISE KEEP: the kept customisation could not be set aside"
            );
            assert_eq!(contents_of(&session.kept_file), kept);
            assert_eq!(contents_of(&session.backup().join("inside")), "still here");
            assert!(!session.state().kept);
        }
    }

    // What a load reports of the names the rules ask for that nothing defines
    // includes the built-in's own, after every load of anything. They say so, so
    // that a front end can show them once, as a count.
    #[test]
    fn names_the_built_in_itself_leaves_undefined_say_they_are_the_built_ins_in_a_loads_reply() {
        const GHOSTLY: &str = r#"{
  "format": "katana-customisation", "version": 1, "name": "Ghostly",
  "codes": [
    {"key": "GH*", "sets": "feature", "layer": "GHOSTS", "linestyle": "GHOST Line"}
  ]
}"#;
        let mut session = Hosted::with("ghost", false, true);
        session.host.built_in = built_in_from(GHOSTLY);
        session.start();
        // Site's GT* asks for SITE Gate, which nothing defines; the built-in's own
        // GH* asks for GHOST Line, which nothing defines either: 2 names, one the
        // built-in's.
        let site = session.scratch.write("site.json", SITE);
        let reply = session.ok(&format!("CUSTOMISE {}", typed(&site)));
        assert!(
            reply.contains("undefined names=2 in_built_in=1\n"),
            "{reply}"
        );
        assert!(
            reply.contains("undefined name=\"GHOST Line\" in_built_in=yes\n"),
            "{reply}"
        );
        assert!(reply.contains("undefined name=\"SITE Gate\"\n"), "{reply}");
        assert!(!reply.contains("\"SITE Gate\" in_built_in"), "{reply}");

        // A built-in with nothing undefined adds no word to the reply at all
        // (what every other test here reads).
        let mut plain = Hosted::new("ghost-none");
        plain.start();
        let plain_site = plain.scratch.write("site.json", SITE);
        let other = plain.ok(&format!("CUSTOMISE {}", typed(&plain_site)));
        assert!(!other.contains("in_built_in"), "{other}");
        assert!(other.contains("undefined names=1\n"), "{other}");
    }

    #[test]
    fn a_write_that_fails_is_refused_and_leaves_what_was_there() {
        // EXPORT to a name a folder has: the text is written beside it and cannot
        // be put in its place. The folder stands, and nothing is left beside it.
        {
            let mut session = Hosted::new("write-export");
            session.start();
            let folder = session.scratch.root.join("a-folder");
            fs::create_dir_all(&folder).expect("the folder is made");
            let error = session.refused(&format!("CUSTOMISE EXPORT {}", typed(&folder)));
            assert_eq!(error.code, ErrorCode::FileExportFailure);
            assert_eq!(error.message, "the file cannot be written");
            assert!(error.context.contains("a-folder"), "{}", error.context);
            assert!(folder.is_dir());
            assert!(!beside(&folder, ".tmp").exists());
        }
        // KEEP where the kept file's folder cannot be made: a file stands where
        // the folder would be. The session stays not kept.
        {
            let mut session = Hosted::new("write-keep");
            let was = "a file, where the kept file's folder would be";
            let blocker = session.scratch.write("blocker", was);
            session.kept_file = blocker.join("customisation.json");
            session.host.kept_file = session.kept_file.clone();
            session.start();
            session.ok("CUSTOMISE SET auto.codes=off");
            let error = session.refused("CUSTOMISE KEEP");
            assert_eq!(error.code, ErrorCode::FileExportFailure);
            assert_eq!(error.message, "the file cannot be written");
            assert_eq!(contents_of(&blocker), was);
            assert!(!session.state().kept);
        }
        // KEEP where the file that is there cannot be kept beside it: a folder
        // has the .bak's name. Nothing is written - the kept file is as it was.
        {
            let mut session = Hosted::new("write-backup");
            session.start();
            session.ok("CUSTOMISE SET auto.codes=off");
            session.ok("CUSTOMISE KEEP");
            let first = contents_of(&session.kept_file);
            fs::create_dir_all(session.backup()).expect("the folder is made");
            session.ok("CUSTOMISE SET linework.start=S");
            let error = session.refused("CUSTOMISE KEEP");
            assert_eq!(error.code, ErrorCode::FileExportFailure);
            assert_eq!(
                error.message,
                "the file that is there could not be kept beside it, so nothing was written"
            );
            assert_eq!(contents_of(&session.kept_file), first);
            assert!(!beside(&session.kept_file, ".tmp").exists());
            assert!(!session.state().kept);
        }
    }

    #[test]
    fn keep_refuses_a_session_with_no_name_until_an_export_and_a_replace_name_it() {
        // Rules made with no customisation ever installed - an editor's, on an
        // empty session: there is a customisation, and it has no name.
        let mut session = Hosted::with("keep-unnamed", false, true);
        session.start();
        session.document.set_survey_map(parsed(SITE).map);
        assert_eq!(session.state().name, "");

        let error = session.refused("CUSTOMISE KEEP");
        assert_eq!(error.code, ErrorCode::InvalidState);
        assert_eq!(
            error.message,
            concat!(
                "CUSTOMISE KEEP: the session's customisation has no name to be kept under; ",
                "CUSTOMISE EXPORT <file> NAME <name> writes it under one, and CUSTOMISE REPLACE ",
                "<file> then gives the session that name"
            )
        );
        assert!(!session.kept_file.exists());

        // Done as it says: the session is "Mine", and is kept. Site's 3 rules
        // over 3 keys, and no definition - only its rules were set.
        let file = typed(&session.scratch.root.join("mine.json"));
        session.ok(&format!("CUSTOMISE EXPORT {file} NAME Mine"));
        session.ok(&format!("CUSTOMISE REPLACE {file}"));
        assert_eq!(session.state().name, "Mine");
        let kept = record(&session.ok("CUSTOMISE KEEP"), "kept", 0);
        assert_eq!(field(&kept, "written"), "yes");
        assert_eq!(field(&kept, "name"), "Mine");
        assert_eq!(field(&kept, "definitions"), "0");
        assert_eq!(field(&kept, "codes"), "3");
        assert_eq!(field(&kept, "rules"), "3");
        assert_eq!(parsed(&contents_of(&session.kept_file)).name, "Mine");
    }

    #[test]
    fn with_no_host_reset_keep_and_revert_are_refused_by_name_and_the_rest_works() {
        let scratch = Scratch::new("no-host");
        let mut session = Bare::default();
        session.load_built_in(&scratch);
        let before = session.document.customisation();
        for word in ["RESET", "KEEP", "REVERT"] {
            let error = session.refused(&format!("CUSTOMISE {word}"));
            assert_eq!(error.code, ErrorCode::InvalidState, "{word}");
            assert!(
                error
                    .message
                    .starts_with(&format!("CUSTOMISE {word} needs ")),
                "{}",
                error.message
            );
        }
        assert!(session.document.customisation() == before);
        // KEEP and REVERT say what would name a kept file.
        assert!(
            session
                .refused("CUSTOMISE KEEP")
                .message
                .contains("KATANA_CUSTOMISATION")
        );

        // Everything else is a session's own.
        assert!(
            session
                .ok("CUSTOMISE")
                .starts_with("2 linestyle and symbol definitions")
        );
        assert!(session.ok("CUSTOMISE JSON").starts_with("{\n"));
        session.ok(&format!(
            "CUSTOMISE {}",
            typed(&scratch.write("site.json", SITE))
        ));
        session.ok("CUSTOMISE SET auto.codes=off");
        session.ok("CUSTOMISE REMOVE CODE GT*");
        session.ok("CUSTOMISE REMOVE \"SITE Tree\" FORCE");
        let out = scratch.root.join("out.json");
        session.ok(&format!("CUSTOMISE EXPORT {}", typed(&out)));
        assert_eq!(parsed(&contents_of(&out)).map.size(), 3);
    }

    #[test]
    fn a_host_with_no_built_in_or_no_kept_file_refuses_the_verb_that_needs_it() {
        // A build with no built-in: RESET has nothing to give.
        let mut no_built_in = Hosted::with("no-built-in", false, true);
        no_built_in.start();
        assert_eq!(
            no_built_in.refused("CUSTOMISE RESET").message,
            "CUSTOMISE RESET needs a built-in customisation, and this program has none"
        );
        // Nothing is loaded, so nothing can be kept, and nothing is kept to go
        // back to.
        assert_eq!(
            no_built_in.refused("CUSTOMISE KEEP").message,
            "CUSTOMISE KEEP: no customisation is loaded, so there is nothing to keep"
        );
        let revert = no_built_in.refused("CUSTOMISE REVERT");
        assert_eq!(revert.code, ErrorCode::NotFound);
        assert!(
            revert.message.contains("no customisation is kept"),
            "{}",
            revert.message
        );
        assert!(!no_built_in.kept_file.exists());

        // A built-in that was asked for and did not read: RESET says why.
        let mut damaged = Hosted::with("damaged-built-in", false, false);
        damaged.host.built_in.problem =
            String::from("the built-in customisation does not read: line 3");
        damaged.start();
        assert_eq!(
            damaged.refused("CUSTOMISE RESET").message,
            concat!(
                "CUSTOMISE RESET cannot use the built-in customisation: the built-in ",
                "customisation does not read: line 3"
            )
        );

        // A front end that keeps nothing (katana_cli without the variable).
        let mut no_kept = Hosted::with("no-kept-file", true, false);
        no_kept.start();
        for word in ["KEEP", "REVERT"] {
            let error = no_kept.refused(&format!("CUSTOMISE {word}"));
            assert_eq!(error.code, ErrorCode::InvalidState, "{word}");
            assert_eq!(
                error.message,
                format!(
                    "CUSTOMISE {word} needs a kept customisation file, and this session has \
                     none; the environment variable KATANA_CUSTOMISATION names one"
                )
            );
        }
        // RESET needs only the built-in, and with no kept file the session is
        // what the next start gives.
        no_kept.ok("CUSTOMISE SET auto.codes=off");
        assert_eq!(
            no_kept.ok("CUSTOMISE RESET"),
            "reset name=\"Small Built In\" definitions=2 codes=2 rules=2 kept=yes"
        );
    }

    // ---- remove ----------------------------------------------------------------------------

    #[test]
    fn removing_a_definition_in_use_is_refused_saying_who_uses_it_unless_forced() {
        let mut session = Hosted::new("remove");
        session.start();
        // A style of the drawing names the fence as its linetype; the FE* rule
        // (rule 0, a feature rule) names it as its linestyle.
        session.ok("STYLE NEW Fence");
        session.ok("STYLE SET Fence linetype \"TEST Fence\"");

        let error = session.refused("CUSTOMISE REMOVE \"TEST Fence\"");
        assert_eq!(error.code, ErrorCode::InvalidState);
        assert_eq!(
            error.message,
            concat!(
                "CUSTOMISE REMOVE: 1 of the 1 definition named is in use, so nothing was ",
                "removed; FORCE removes what is used too, and what names it then draws plain\n",
                "  \"TEST Fence\": rule #0 FE* (feature) names it\n",
                "  \"TEST Fence\": the drawing's style \"Fence\" names it"
            )
        );
        assert!(session.document.style_library().contains("TEST Fence"));
        assert_eq!(session.state().origin, CustomisationOrigin::BuiltIn);

        // The peg is named by the PG* rule (rule 1, a symbol rule) as its symbol.
        // One used and one not, in one line: neither goes.
        add_definition(&mut session.document, "TEST Spare", false);
        let two = session.refused("CUSTOMISE REMOVE \"TEST Spare\" \"TEST Peg\"");
        assert_eq!(
            two.message,
            concat!(
                "CUSTOMISE REMOVE: 1 of the 2 definitions named are in use, so nothing was ",
                "removed; FORCE removes what is used too, and what names it then draws plain\n",
                "  \"TEST Peg\": rule #1 PG* (symbol) names it"
            )
        );
        assert_eq!(session.document.style_library().size(), 3);

        // One nothing names goes without FORCE; a name given twice is one name.
        assert_eq!(
            session.ok("CUSTOMISE REMOVE \"TEST Spare\" \"TEST Spare\""),
            "removed definition=\"TEST Spare\" rules=0 styles=0 layers=0"
        );
        // Forced, the reply still says who names what is gone.
        assert_eq!(
            session.ok("CUSTOMISE REMOVE \"TEST Fence\" \"TEST Peg\" force"),
            concat!(
                "removed definition=\"TEST Fence\" rules=1 styles=1 layers=0\n",
                "removed definition=\"TEST Peg\" rules=1 styles=0 layers=0"
            )
        );
        assert!(session.document.style_library().empty());
        assert_eq!(
            session.document.survey_map().size(),
            2,
            "the rules are not removed with them"
        );
        assert_eq!(session.state().origin, CustomisationOrigin::Edited);
        assert!(!session.state().kept);
    }

    #[test]
    fn a_layers_linetype_and_a_styles_symbol_are_uses_of_a_definition_too() {
        // Two definitions no rule names: a linestyle a LAYER is drawn with, and a
        // symbol a STYLE draws its points with. An entity with no style of its
        // own is drawn with its layer's linetype, so the layer holds the
        // definition in place exactly as a style does - and was once not asked:
        // the definition went without FORCE and without a word.
        let mut session = Hosted::new("remove-layer");
        session.start();
        add_definition(&mut session.document, "TEST Hedge", false);
        add_definition(&mut session.document, "TEST Stake", true);
        session.ok("LAYER NEW trees");
        session.ok("LAYER LTYPE trees \"TEST Hedge\"");
        session.ok("STYLE NEW Mark");
        session.ok("STYLE SET Mark symbol \"TEST Stake\"");

        let hedge = session.refused("CUSTOMISE REMOVE \"TEST Hedge\"");
        assert_eq!(hedge.code, ErrorCode::InvalidState);
        assert_eq!(
            hedge.message,
            concat!(
                "CUSTOMISE REMOVE: 1 of the 1 definition named is in use, so nothing was ",
                "removed; FORCE removes what is used too, and what names it then draws plain\n",
                "  \"TEST Hedge\": the drawing's layer \"trees\" names it"
            )
        );
        let stake = session.refused("CUSTOMISE REMOVE \"TEST Stake\"");
        assert_eq!(
            stake.message,
            concat!(
                "CUSTOMISE REMOVE: 1 of the 1 definition named is in use, so nothing was ",
                "removed; FORCE removes what is used too, and what names it then draws plain\n",
                "  \"TEST Stake\": the drawing's style \"Mark\" names it"
            )
        );
        assert_eq!(session.document.style_library().size(), 4);

        assert_eq!(
            session.ok("CUSTOMISE REMOVE \"TEST Hedge\" \"TEST Stake\" FORCE"),
            concat!(
                "removed definition=\"TEST Hedge\" rules=0 styles=0 layers=1\n",
                "removed definition=\"TEST Stake\" rules=0 styles=1 layers=0"
            )
        );
        assert_eq!(session.document.style_library().size(), 2);
    }

    #[test]
    fn a_style_that_names_a_definition_both_ways_is_cited_and_counted_once() {
        // REMOVE asks cad's definition_users who names a definition - the
        // function the window's definition editor lists them with. That answer
        // says HOW each user names it, so a style whose linetype AND symbol are
        // the definition is in it twice; the verb says only THAT a style names
        // it. Three styles, made as a file's are (no verb checks what a style
        // names): Alpha draws its points with the stake, Both names it both
        // ways, Zed names it as its linetype. By hand: 3 styles, each cited once,
        // in name order.
        let mut session = Hosted::new("remove-both-ways");
        session.start();
        add_definition(&mut session.document, "TEST Stake", true);
        add_style(&mut session, "Zed", "TEST Stake", "");
        add_style(&mut session, "Both", "TEST Stake", "TEST Stake");
        add_style(&mut session, "Alpha", "", "TEST Stake");
        let users = definition_users(&session.document, "TEST Stake");
        assert_eq!(users.linetype_styles, ["Both", "Zed"]);
        assert_eq!(users.symbol_styles, ["Alpha", "Both"]);

        let refusal = session.refused("CUSTOMISE REMOVE \"TEST Stake\"");
        assert_eq!(refusal.code, ErrorCode::InvalidState);
        assert_eq!(
            refusal.message,
            concat!(
                "CUSTOMISE REMOVE: 1 of the 1 definition named is in use, so nothing was ",
                "removed; FORCE removes what is used too, and what names it then draws plain\n",
                "  \"TEST Stake\": the drawing's style \"Alpha\" names it\n",
                "  \"TEST Stake\": the drawing's style \"Both\" names it\n",
                "  \"TEST Stake\": the drawing's style \"Zed\" names it"
            )
        );
        assert_eq!(
            session.ok("CUSTOMISE REMOVE \"TEST Stake\" FORCE"),
            "removed definition=\"TEST Stake\" rules=0 styles=3 layers=0"
        );
    }

    #[test]
    fn the_line_an_editor_builds_to_remove_a_definition_is_read_as_that_definition_alone() {
        // The window's definition editor does not remove a definition: it runs
        // the line definition_edit's remove_definition_line builds, and this
        // verb reads it. The two were written side by side, so here they meet:
        // every name that function will put on a line is read back by the verb
        // as that one definition - and the names it refuses to put on one are
        // exactly the words this verb would read as its own.
        //
        // By hand from the tokenizer's contract (annotation/command_words.rs:
        // it splits on blanks, groups quoted words, removes the quotes and has no
        // escape) and from this family's grammar (CODE is a keyword only as
        // REMOVE's first word, FORCE only as its last; every other word of the
        // family is a keyword only as CUSTOMISE's first). So inside the quotes a
        // name may hold anything but a double quote or a line break - blanks at
        // its ends, two in a row, '#', ';', '=', an apostrophe, text outside
        // ASCII - and may BE any word of the family but those two.
        let mut session = Bare::default();
        let names = [
            "TEST Valve",
            " led by a blank",
            "two  blanks",
            "ends in one ",
            "REMOVE",
            "SET",
            "ONLY",
            "JSON",
            "replace",
            "CODES",
            "KEEP",
            "Code Red",
            "Forced",
            "FORCE main",
            "main FORCE",
            "7 up",
            "a#b",
            "#first",
            "a;b",
            "auto.codes=on",
            "it's",
            "\u{dc}ber Mark",
        ];
        for name in names {
            add_definition(&mut session.document, name, false);
        }
        assert_eq!(session.document.style_library().size(), names.len());

        let mut left = names.len();
        let mut force = false;
        for name in names {
            // Plain and forced by turns: both lines are the editor's.
            let line = remove_definition_line(name, force)
                .unwrap_or_else(|error| panic!("{name}: {}", error.describe()));
            assert_eq!(
                line,
                format!(
                    "CUSTOMISE REMOVE \"{name}\"{}",
                    if force { " FORCE" } else { "" }
                )
            );
            let reply = session.ok(&line);
            assert!(reply.starts_with("removed definition="), "{name}: {reply}");
            assert!(
                reply.ends_with(" rules=0 styles=0 layers=0"),
                "{name}: {reply}"
            );
            assert!(!session.document.style_library().contains(name), "{name}");
            left -= 1;
            assert_eq!(session.document.style_library().size(), left, "{name}");
            force = !force;
        }

        // The two words no line can carry as a name, in any case - and why: the
        // verb reads each as its own word, so the line an editor would have built
        // does something else or nothing. A library can hold such a name, out of
        // a file.
        for word in ["CODE", "code", "FORCE", "Force"] {
            add_definition(&mut session.document, word, false);
            for forced in [false, true] {
                let refused = remove_definition_line(word, forced).expect_err(word);
                assert_eq!(refused.code, ErrorCode::InvalidArgument, "{word}");
            }
            // Written out all the same, the plain line is a usage refusal: CODE
            // with no key after it, or FORCE with no definition before it.
            let plain = session.refused(&format!("CUSTOMISE REMOVE \"{word}\""));
            assert_eq!(plain.code, ErrorCode::InvalidArgument, "{word}");
            assert!(
                plain.message.starts_with("usage: CUSTOMISE REMOVE "),
                "{}",
                plain.message
            );
            assert!(session.document.style_library().contains(word), "{word}");
        }
        // And "remove the definition CODE anyway" is read as "remove the survey
        // code FORCE", which no rule has.
        let misread = session.refused("CUSTOMISE REMOVE \"CODE\" FORCE");
        assert_eq!(misread.code, ErrorCode::NotFound);
        assert_eq!(misread.context, "\"FORCE\"");
        assert!(session.document.style_library().contains("CODE"));
    }

    #[test]
    fn removing_what_is_not_there_removes_nothing() {
        let mut session = Hosted::new("remove-unknown");
        session.start();
        let error = session.refused("CUSTOMISE REMOVE \"TEST Peg\" Nowhere \"Not Here\" FORCE");
        assert_eq!(error.code, ErrorCode::NotFound);
        assert_eq!(error.context, "\"Nowhere\", \"Not Here\"");
        assert_eq!(session.document.style_library().size(), 2);

        let code = session.refused("CUSTOMISE REMOVE CODE PG* pg* ZZ*");
        assert_eq!(code.code, ErrorCode::NotFound);
        // A key is matched as it is written, as a code is: pg* is not PG*.
        assert_eq!(code.context, "\"pg*\", \"ZZ*\"");
        assert_eq!(session.document.survey_map().size(), 2);

        for line in [
            "CUSTOMISE REMOVE",
            "CUSTOMISE REMOVE CODE",
            "CUSTOMISE REMOVE FORCE",
        ] {
            let usage = session.refused(line);
            assert_eq!(usage.code, ErrorCode::InvalidArgument, "{line}");
            assert!(
                usage.message.starts_with("usage: CUSTOMISE REMOVE "),
                "{}",
                usage.message
            );
        }
        assert_eq!(session.state().origin, CustomisationOrigin::BuiltIn);
    }

    #[test]
    fn remove_code_takes_every_rule_of_each_key() {
        // Site merged in: FE* feature, PG* symbol, TR* symbol, GT* feature. A
        // second FE* rule in another section, so that one key has two.
        let mut session = Hosted::new("remove-code");
        session.start();
        let site = session.scratch.write("site.json", SITE);
        session.ok(&format!("CUSTOMISE {}", typed(&site)));
        let mut map = session.document.survey_map().clone();
        let surface = SurveyRule {
            key: String::from("FE*"),
            section: SurveySection::Tinable,
            tinable: Some(true),
            ..SurveyRule::default()
        };
        assert!(map.add(surface).is_ok());
        session.document.set_survey_map(map);
        assert_eq!(session.document.survey_map().size(), 5);

        assert_eq!(
            session.ok("CUSTOMISE REMOVE CODE FE* GT* FE*"),
            "removed code=FE* rules=2\nremoved code=GT* rules=1"
        );
        let rules = session.document.survey_map().rules();
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].key, "PG*");
        assert_eq!(rules[1].key, "TR*");
        assert_eq!(
            session.document.style_library().size(),
            3,
            "definitions are not codes"
        );
    }

    // ---- settings --------------------------------------------------------------------------

    #[test]
    fn set_changes_the_switches_and_the_control_codes_and_says_them_back() {
        let mut session = Hosted::new("set");
        session.start();
        // Keys in any case; the reply in the form the keys are documented in.
        assert_eq!(
            session.ok(concat!(
                "CUSTOMISE SET auto.codes=off AUTO.LINEWORK=Off linework.start=S ",
                "linework.arcStart=PC"
            )),
            "set auto.codes=off auto.linework=off linework.start=S linework.arcstart=PC"
        );
        assert!(!session.state().automation.codes_on_survey_import);
        assert!(!session.state().automation.linework_on_survey_import);
        assert_eq!(session.state().linework.start, "S");
        assert_eq!(session.state().linework.arc_start, "PC");
        assert_eq!(
            session.state().linework.end,
            "END",
            "what was not named is as it was"
        );
        assert_eq!(session.state().origin, CustomisationOrigin::Edited);
        assert!(!session.state().kept);

        // An empty spelling switches a control off.
        assert_eq!(
            session.ok("CUSTOMISE SET linework.join= auto.codes=on"),
            "set linework.join=\"\" auto.codes=on"
        );
        assert_eq!(session.state().linework.join, "");
        assert!(session.state().automation.codes_on_survey_import);

        // The report says both in the words SET takes, so either line can be
        // typed back - and typed back, changes nothing.
        let report = session.ok("CUSTOMISE");
        assert!(
            report.contains("automation auto.codes=on auto.linework=off\n"),
            "{report}"
        );
        let linework = concat!(
            "linework.start=S linework.end=END linework.close=CL linework.arcstart=PC ",
            "linework.arcend=EC linework.join=\"\" linework.rectangle=RECT"
        );
        assert!(
            report.contains(&format!("linework {linework}\n")),
            "{report}"
        );
        let generation = session.document.customisation_generation();
        session.ok(&format!(
            "CUSTOMISE SET {linework} auto.codes=on auto.linework=off"
        ));
        assert_eq!(session.document.customisation_generation(), generation);
    }

    #[test]
    fn one_refused_item_sets_none_of_them() {
        let mut session = Hosted::new("set-refused");
        session.start();
        let generation = session.document.customisation_generation();
        // Two controls spelled alike mean neither; a spelling with a blank can
        // never be met in a field code. Both are refused by the one judge of
        // linework codes, and the switch named beside them is not turned.
        for line in [
            "CUSTOMISE SET auto.codes=off linework.start=END",
            "CUSTOMISE SET auto.codes=off linework.close=end",
            "CUSTOMISE SET auto.codes=off \"linework.start=A B\"",
        ] {
            let error = session.refused(line);
            assert_eq!(error.code, ErrorCode::InvalidArgument, "{line}");
            assert!(
                error.message.starts_with(
                    "CUSTOMISE SET: the linework codes would be refused, so nothing was set: "
                ),
                "{}",
                error.message
            );
        }
        // What is not a key, not a value, or not an item at all.
        assert_eq!(
            session
                .refused("CUSTOMISE SET auto.linework=off auto.codes=maybe")
                .message,
            "auto.codes is on or off"
        );
        assert!(
            session
                .refused("CUSTOMISE SET auto.codes=off linework.finish=F")
                .message
                .starts_with("not a SET key; usage: CUSTOMISE SET ")
        );
        assert!(
            session
                .refused("CUSTOMISE SET auto.codes=off name=Mine")
                .message
                .starts_with("not a SET key; ")
        );
        assert!(
            session
                .refused("CUSTOMISE SET auto.codes=off linework.start")
                .message
                .starts_with("a SET item is key=value; ")
        );
        // A value with no key before its '='.
        assert!(
            session
                .refused("CUSTOMISE SET auto.codes=off =S")
                .message
                .starts_with("a SET item is key=value; ")
        );
        assert!(
            session
                .refused("CUSTOMISE SET")
                .message
                .starts_with("usage: CUSTOMISE SET ")
        );
        // A key given twice is two answers to one question, whatever the case
        // of its letters: neither is taken. (The later once won, and the reply
        // said both back.)
        let twice = session.refused("CUSTOMISE SET linework.start=S linework.start=XX");
        assert_eq!(twice.code, ErrorCode::InvalidArgument);
        assert_eq!(
            twice.message,
            "a SET key is given once in a line, and linework.start is given twice"
        );
        assert_eq!(twice.context, "linework.start=XX");
        assert_eq!(
            session
                .refused("CUSTOMISE SET auto.codes=off AUTO.CODES=on")
                .message,
            "a SET key is given once in a line, and auto.codes is given twice"
        );

        assert!(session.state().automation.codes_on_survey_import);
        assert!(session.state().automation.linework_on_survey_import);
        assert_eq!(session.state().linework.start, "ST");
        assert_eq!(session.document.customisation_generation(), generation);
        assert!(session.state().kept);
    }

    // ---- help ------------------------------------------------------------------------------

    // The one-line index is what an agent reads first (`HELP`, the MCP tool's
    // help): a verb it does not name is a verb that is found only by chance.
    #[test]
    fn the_one_line_help_index_names_customise_built_in() {
        let help = CommandInterpreter::help_text();
        assert!(help.contains("CUSTOMISE BUILTIN [<name>]"), "the index row");
        // On the family's own rows, beside RESET, whose words it explains.
        let built_in = help
            .find("CUSTOMISE BUILTIN [<name>]")
            .expect("the index row");
        let reset = help
            .find("CUSTOMISE RESET (the built-in one)")
            .expect("RESET's row");
        // The C++'s size_t difference: an index row AFTER RESET's wraps round
        // and fails, as it should.
        assert!(reset.wrapping_sub(built_in) < 200);
    }

    // The default is the file THIS build was given, so the help reads its name from
    // what was compiled in and writes none: a build whose default is another file
    // would otherwise say a thing that is not true.
    #[test]
    fn help_customise_names_the_default_this_build_compiled_in() {
        let help = customisation_verb_help();
        assert!(!help.contains("@DEFAULT@"), "the marker is replaced");
        let compiled_in = compiled_in_customisation();
        match &compiled_in.customisation {
            Some(customisation) => assert!(help.contains(&format!(
                "the default, {}, unless KATANA_BUILTIN_CUSTOMISATION says another",
                customisation.name
            ))),
            None => assert!(help.contains(
                "the default (this build has none), unless KATANA_BUILTIN_CUSTOMISATION says \
                 another"
            )),
        }
    }

    #[test]
    fn the_help_names_every_word_and_help_customise_says_every_reply() {
        let help = CommandInterpreter::help_text();
        // The literal the session's own help carried, so that what pins it there
        // (the cli and MCP help tests) passes on this block alone once the
        // session's goes.
        assert!(help.contains("Customise CUSTOMISE [REPLACE] <file> [<file>...]"));
        for word in [
            "CUSTOMISE JSON",
            "CUSTOMISE DEFINITIONS <file> [<file>...]",
            "CUSTOMISE EXPORT <file> [CODES] [LINESTYLES] [SYMBOLS]",
            "CUSTOMISE RESET",
            "KEEP",
            "REVERT",
            "CUSTOMISE REMOVE name... [FORCE]",
            "REMOVE CODE key...",
            "CUSTOMISE SET auto.codes=on|off",
            "HELP CUSTOMISE",
        ] {
            assert!(help.contains(word), "{word}");
        }

        let mut session = Bare::default();
        let family = session
            .interpreter
            .run(&mut session.document, "HELP CUSTOMISE")
            .unwrap_or_default();
        assert_eq!(family, customisation_verb_help());
        assert_eq!(
            session
                .interpreter
                .run(&mut session.document, "help customize")
                .unwrap_or_default(),
            family
        );
        for reply in [
            "customisation name=",
            "source name=",
            "automation auto.codes=",
            "linework linework.start=",
            "missing",
            "loaded file=",
            "removed definitions=",
            "undefined names=",
            "exported file=",
            "left rules= linework=yes|no automation=yes|no",
            "reset name=",
            "CUSTOMISE BUILTIN <name>",
            "builtins count=",
            "used name=",
            "kept file=",
            "written=no reason=the-session-is-the-built-in",
            "reverted file=",
            "removed definition=",
            "removed code=",
            "set <key>=<value>",
            "./json",
            "CUSTOMIZE",
            // What the family says of its own edges: a layer is a use, the file
            // comes first, a key once, and what a part lists.
            "rules= styles= layers=",
            "(./CODES)",
            "A key is given once in a line",
            "lists as its sources only those it holds something of",
            "of either kind unless LINESTYLES or SYMBOLS says which",
        ] {
            assert!(family.contains(reply), "{reply}");
        }
        // Neither names another program's file kinds, nor its name - written in
        // two pieces, so that this file does not hold it.
        let other_program = ["1", "2d"].concat();
        for text in [&help, &family] {
            assert!(!text.contains(".mapfile"));
            assert!(!text.contains(".4d"));
            assert!(!text.contains(&other_program));
        }
    }
}
