// SHEETS APPEND path (tests/cad/plotting/test_sheets_append.cpp;
// plotting/sheet_verbs.rs, docs/plotting.md "Sheets on the command line"):
// another set's sheets, read from the JSON file SHEETS SAVE wrote, put after
// the sheets there are as ONE undoable step - what the Sheets editor's Sheet
// Set > Append Sheets From File runs. The appended sheets are renumbered as a
// generator's are (prepare_for_append); the set keeps its own title block,
// revisions and page setup; and what cannot be appended changes nothing and
// says why.

use std::collections::BTreeSet;
use std::path::Path;

use katana_cad::plotting::SheetSet;
use katana_cad::plotting::generators::prepare_for_append;
use katana_cad::plotting::sheet_verbs::{read_sheet_set_file, write_sheet_set_file};
use katana_core::ErrorCode;

use crate::{ScratchDirectory, Session, contains, must, quoted, write_file};

// A set of tiles with their key plan, as GENERATE grid lays them: 400 x 200
// m at 1:500 with 10 m of overlap is six tiles and a key plan (the verb
// tests count them), saved to `file`. The drawing is empty, so empty=on
// keeps the tiles.
#[track_caller]
fn save_tiles(file: &Path) {
    let mut tiles = Session::default();
    tiles.ok("GENERATE grid area=0,0,400,200 scale=500 overlap=10 empty=on");
    tiles.ok("TITLEBLOCK organisation \"Tile Office\"");
    tiles.ok(&format!("SHEETS SAVE {}", quoted(file)));
}

// Every sheet and view id of `set`, failing when one is there twice; the
// count of views.
#[track_caller]
fn distinct_ids(set: &SheetSet) -> (BTreeSet<String>, usize) {
    let mut ids = BTreeSet::new();
    let mut views = 0;
    for sheet in &set.sheets {
        assert!(ids.insert(sheet.id.clone()), "{}", sheet.id);
        for viewport in &sheet.viewports {
            assert!(ids.insert(viewport.id.clone()), "{}", viewport.id);
            views += 1;
        }
    }
    (ids, views)
}

mod sheets_append {
    use super::*;

    #[test]
    fn another_sets_sheets_go_after_these_in_one_step() {
        let scratch = ScratchDirectory::new("sheets-append-one-step");
        let file = scratch.path.join("tile set.json");
        save_tiles(&file);
        let saved = must(read_sheet_set_file(&file));
        assert_eq!(saved.sheets.len(), 7);

        let mut s = Session::default();
        s.ok("SHEET NEW COVER");
        s.ok("SHEET NEW NOTES");
        s.ok("VIEW ADD 2 notes");
        s.ok("TITLEBLOCK organisation \"Home Office\"");
        s.ok("SHEETS PAGESETUP style=grey");
        let before = (*s.set()).clone();
        let steps = s.steps();

        let reply = s.ok(&format!("SHEETS APPEND {}", quoted(&file)));
        assert_eq!(
            reply.split('\n').next().unwrap_or_default(),
            format!("appended 7 sheets from {}: 3 to 9", quoted(&file))
        );
        assert!(contains(&reply, "\nsheet 3 id="), "{reply}");
        assert!(contains(&reply, "\nsheet 9 id="), "{reply}");
        assert_eq!(s.steps(), steps + 1);
        assert_eq!(s.document.history().undo_name(), "APPEND_SHEETS");

        // The first two are as they were; the rest are the file's, renumbered
        // as a generator's output is when it joins a set.
        let set = s.set();
        assert_eq!(set.sheets.len(), 9);
        assert_eq!(set.sheets[0], before.sheets[0]);
        assert_eq!(set.sheets[1], before.sheets[1]);
        let mut expected = saved.sheets.clone();
        prepare_for_append(&before, &mut expected);
        for (i, sheet) in expected.iter().enumerate() {
            assert_eq!(set.sheets[2 + i], *sheet, "sheet {}", 3 + i);
        }
        // Every sheet and view id is still one of a kind.
        let (ids, views) = distinct_ids(&set);
        assert_eq!(ids.len(), 9 + views);
        // The set keeps its own title block and page setup.
        assert_eq!(set.defaults, before.defaults);
        assert_eq!(set.page_setup, before.page_setup);
        assert_eq!(set.revisions, before.revisions);

        assert!(s.document.undo().is_ok());
        assert_eq!(*s.set(), before);
    }

    #[test]
    fn the_same_file_appended_twice_gives_two_sets_of_sheets_no_id_twice() {
        let scratch = ScratchDirectory::new("sheets-append-twice");
        let file = scratch.path.join("tiles.json");
        save_tiles(&file);
        let mut s = Session::default();
        s.ok(&format!("SHEETS APPEND {}", quoted(&file)));
        s.ok(&format!("SHEETS APPEND {}", quoted(&file)));
        assert_eq!(s.set().sheets.len(), 14);
        distinct_ids(&s.set());
        // The same lines give the same set every time.
        let mut again = Session::default();
        again.ok(&format!("SHEETS APPEND {}", quoted(&file)));
        again.ok(&format!("SHEETS APPEND {}", quoted(&file)));
        assert_eq!(*again.set(), *s.set());
    }

    #[test]
    fn what_cannot_be_appended_changes_nothing_and_says_why() {
        let scratch = ScratchDirectory::new("sheets-append-refused");
        let mut s = Session::default();
        s.ok("SHEET NEW PLAN");
        let before = (*s.set()).clone();
        let steps = s.steps();

        assert_eq!(
            s.refused(&format!(
                "SHEETS APPEND {}",
                quoted(&scratch.path.join("none.json"))
            ))
            .code,
            ErrorCode::NotFound
        );
        write_file(&scratch.path.join("junk.json"), b"not json");
        assert_eq!(
            s.refused(&format!(
                "SHEETS APPEND {}",
                quoted(&scratch.path.join("junk.json"))
            ))
            .code,
            ErrorCode::ParseFailure
        );
        write_file(
            &scratch.path.join("newer.json"),
            br#"{"format": "katana-sheets", "version": 99}"#,
        );
        assert_eq!(
            s.refused(&format!(
                "SHEETS APPEND {}",
                quoted(&scratch.path.join("newer.json"))
            ))
            .code,
            ErrorCode::Unsupported
        );
        // An empty set appends nothing, which is said rather than made a step.
        must(write_sheet_set_file(
            &SheetSet::default(),
            &scratch.path.join("empty.json"),
        ));
        let empty = s.refused(&format!(
            "SHEETS APPEND {}",
            quoted(&scratch.path.join("empty.json"))
        ));
        assert_eq!(empty.code, ErrorCode::InvalidArgument);
        assert_eq!(empty.message, "the file holds no sheets to append");
        assert!(contains(
            &s.refused("SHEETS APPEND").message,
            "usage: SHEETS"
        ));
        assert!(contains(
            &s.refused("SHEETS APPEND a.json b.json").message,
            "usage: SHEETS"
        ));
        assert_eq!(*s.set(), before);
        assert_eq!(s.steps(), steps);
    }

    #[test]
    fn stored_sheets_that_cannot_be_read_are_not_appended_to() {
        // Appending to sheets that cannot be read would replace them with the
        // file's; only SHEETS LOAD replaces them, on purpose.
        let scratch = ScratchDirectory::new("sheets-append-unreadable");
        let file = scratch.path.join("tiles.json");
        save_tiles(&file);
        let mut s = Session::default();
        let mut metadata = s.document.metadata().clone();
        metadata.unknown_keys.insert(
            String::from("sheets"),
            String::from(r#"{"format": "katana-sheets", "version": 99}"#),
        );
        s.document.set_metadata(metadata);
        assert_eq!(
            s.refused(&format!("SHEETS APPEND {}", quoted(&file))).code,
            ErrorCode::Unsupported
        );
        assert_eq!(
            s.document.metadata().unknown_keys["sheets"],
            r#"{"format": "katana-sheets", "version": 99}"#
        );
    }

    #[test]
    fn the_help_lists_it() {
        let mut s = Session::default();
        assert!(contains(&s.ok("HELP SHEETS"), "SHEETS APPEND path"));
    }
}
