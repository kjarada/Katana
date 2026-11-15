// Storing sheets and editing them undoably (sheet_json.hpp, sheet_commands.hpp,
// Document::sheetSet / setSheetSet); tests/cad/plotting/test_sheet_storage.cpp.
//
// A sheet set lives in the project's metadata under "sheets" as versioned
// JSON, and its logo in the project's assets/ directory - no schema change.
// Every edit is one undoable step.

use std::path::{Path, PathBuf};

use katana_cad::plotting::generators::grid_sheets;
use katana_cad::plotting::sheet_commands::{
    ADD_SHEETS, EDIT_VIEWPORT, MAXIMUM_LOGO_BYTES, add_sheet, add_sheets, duplicate_sheet,
    edit_viewport, field_context_for, frame_date, import_logo, logo_path, move_sheet, remove_sheet,
};
use katana_cad::plotting::sheet_json::{sheet_set_from_json, sheet_set_to_json};
use katana_cad::plotting::sheet_set::mark_label;
use katana_cad::plotting::{
    GridRequest, Revision, Sheet, SheetSet, SignOff, Viewport, ViewportKind, ViewportSource,
    WorldMark, WorldMarkKind,
};
use katana_cad::{Document, PaperSize};
use katana_core::civil_date::YearMonthDay;
use katana_core::error::make_error;
use katana_core::{ErrorCode, Status};
use katana_geometry::{Box2, Point2};

fn ok<T>(result: katana_core::Result<T>) -> T {
    result.unwrap_or_else(|error| panic!("{}", error.describe()))
}

fn text(value: &str) -> String {
    String::from(value)
}

fn sign_off(name: &str, date: &str) -> SignOff {
    SignOff {
        name: text(name),
        date: text(date),
    }
}

fn revision(code: &str, date: &str, description: &str, by: &str) -> Revision {
    Revision {
        code: text(code),
        date: text(date),
        description: text(description),
        by: text(by),
    }
}

// A set with something other than the default in every member, and doubles
// that only survive a round trip written in full: a third, 0.1 + 0.2, a
// value near the bottom of the range, a negative angle.
fn everything() -> SheetSet {
    let mut set = SheetSet::default();
    set.defaults.organisation = text("Example Surveys");
    set.defaults.project_lines = vec![
        text("Main Road Upgrade"),
        text("Stage 2"),
        text("Lot 7 \u{2014} North"),
        text("Detail"),
    ];
    set.defaults.client = text("Client Pty Ltd");
    set.defaults.locator = sign_off("L. OCATOR", "01/01/26");
    set.defaults.surveyor = sign_off("J. CITIZEN", "02/01/26");
    set.defaults.compiler = sign_off("C. OMPILER", "");
    set.defaults.reviewer = sign_off("R. EVIEWER", "04/01/26");
    set.defaults.approver = sign_off("A. PPROVER", "05/01/26");
    set.defaults.notes = text("Services are approximate.\nConfirm before digging.");
    set.defaults.height_datum = text("LOCAL");
    set.defaults.coordinate_system = text("GRID 56");
    set.defaults.model_name = text("GROUND");
    set.defaults.set_number = text("DS-0001");
    set.defaults.logo_asset = text("logo.png");
    set.numbering = text("{set}-{n:02}");
    set.revisions = vec![
        revision("A", "01/02/26", "First issue", "JC"),
        revision("B", "03/02/26", "Kerbs", "RE"),
    ];

    let mut sheet = Sheet {
        id: text("s7"),
        name: text("PLAN \u{5317}"), // a non-ASCII name
        paper: PaperSize::A1,
        landscape: true,
        frame_legend: false,
        ..Sheet::default()
    };
    sheet.fields.insert(text("scale"), text("1:250 @ A1"));
    let mut plan = Viewport {
        id: text("vp9"),
        kind: ViewportKind::Plan,
        rect: Box2::new(Point2::new(46.0, 70.0), Point2::new(820.0 / 3.0, 574.0)),
        scale: 250.0,
        auto_scale: true,
        centre: Point2::new(0.1 + 0.2, 1e-300),
        auto_centre: true,
        rotation: -0.7853981633974483,
        vertical_exaggeration: 2.5,
        tilt_degrees: 45.0,
        source: ViewportSource {
            alignment: text("ROAD"),
            chainage_from: 12.5,
            chainage_to: 250.25,
            section_interval: 20.0,
            stations: vec![10.0, 20.0],
            section_half_width: 15.0,
        },
        title: text("PLAN AT THE CORNER"),
        north_arrow: true,
        scale_bar: true,
        locked: true,
        text: text("note"),
        ..Viewport::default()
    };
    plan.hidden_layers.hide("FRAMES");
    plan.hidden_layers.hide("TREES");
    plan.marks.push(WorldMark {
        kind: WorldMarkKind::MatchLine,
        points: vec![Point2::new(1.0, 2.0), Point2::new(3.0, 4.0)],
        label: text("MATCH LINE CH 250.000"),
        sheet: text("s8"),
    });
    plan.marks.push(WorldMark {
        kind: WorldMarkKind::SheetOutline,
        points: vec![Point2::new(5.0, 6.0)],
        label: String::new(),
        sheet: text("s1"),
    });
    sheet.viewports.push(plan);
    sheet.viewports.push(Viewport {
        id: text("vp10"),
        kind: ViewportKind::Legend,
        ..Viewport::default()
    });
    set.sheets.push(sheet);
    set.sheets.push(Sheet {
        id: text("s8"),
        paper: PaperSize::A4,
        landscape: false,
        frame: String::new(),
        ..Sheet::default()
    });
    set
}

fn named_sheet(name: &str) -> Sheet {
    Sheet {
        name: text(name),
        viewports: vec![Viewport {
            kind: ViewportKind::Plan,
            ..Viewport::default()
        }],
        ..Sheet::default()
    }
}

fn names(document: &Document) -> Vec<String> {
    document
        .sheet_set()
        .sheets
        .iter()
        .map(|sheet| sheet.name.clone())
        .collect()
}

fn has_sheets_key(document: &Document) -> bool {
    document.metadata().unknown_keys.contains_key("sheets")
}

fn json_of(set: &SheetSet) -> String {
    ok(sheet_set_to_json(set))
}

fn read(json: &str) -> SheetSet {
    ok(sheet_set_from_json(json))
}

// A directory of the test's own, removed by name when it goes: the tests of
// one binary run at the same time, and a shared one would be removed from
// under another, so the name carries the test's and the process's.
struct ScratchDirectory {
    path: PathBuf,
}

impl ScratchDirectory {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("katana-cad-tests-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        let _ = std::fs::create_dir_all(&path);
        Self { path }
    }
}

impl Drop for ScratchDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn write_file(path: &Path, bytes: &[u8]) {
    std::fs::write(path, bytes).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
}

// The eight-byte PNG signature and a little after it: enough for a logo to
// be recognised by its content (nothing here decodes the image).
fn png_bytes() -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    bytes.extend_from_slice(b"not really pixels");
    bytes
}

mod sheet_storage {
    use super::*;

    // ---- JSON ----------------------------------------------------------------------------

    #[test]
    fn every_sheet_set_member_survives_a_json_round_trip_exactly() {
        let set = everything();
        let json = json_of(&set);
        assert!(json.contains("\"format\":\"katana-sheets\""));
        assert!(json.contains("\"version\":1"));
        let back = read(&json);
        // Bit for bit: == compares every double exactly.
        assert!(back == set);
        assert_eq!(back.sheets[0].viewports[0].centre.x, 0.1 + 0.2);
        assert_eq!(back.sheets[0].viewports[0].centre.y, 1e-300);
        // And the text is stable: writing what was read gives the same text.
        assert_eq!(json_of(&back), json);
    }

    #[test]
    fn members_left_out_read_as_their_defaults() {
        let set = read(
            r#"{"format":"katana-sheets","version":1,"sheets":[{"viewports":[{"kind":"legend"}]}]}"#,
        );
        assert_eq!(set.sheets.len(), 1);
        let sheet = &set.sheets[0];
        assert_eq!(sheet.paper, PaperSize::A3);
        assert!(sheet.landscape);
        assert_eq!(sheet.frame, katana_cad::plotting::frame::BUILT_IN_FRAME_ID);
        assert_eq!(sheet.viewports.len(), 1);
        assert_eq!(sheet.viewports[0].kind, ViewportKind::Legend);
        assert_eq!(sheet.viewports[0].scale, 500.0);
        assert_eq!(sheet.viewports[0].vertical_exaggeration, 1.0);
        assert_eq!(set.numbering, "{n}");
    }

    #[test]
    fn a_newer_version_is_refused_and_nonsense_is_a_parse_failure() {
        let code = |json: &str| sheet_set_from_json(json).expect_err("not read").code;
        assert_eq!(
            code(r#"{"format":"katana-sheets","version":2}"#),
            ErrorCode::Unsupported
        );
        assert_eq!(code("not json"), ErrorCode::ParseFailure);
        assert_eq!(code(r#"{"version":1}"#), ErrorCode::ParseFailure);
        assert_eq!(
            code(
                r#"{"format":"katana-sheets","version":1,"sheets":[{"viewports":[{"kind":"globe"}]}]}"#
            ),
            ErrorCode::ParseFailure
        );
        assert_eq!(
            code(r#"{"format":"katana-sheets","version":1,"sheets":[{"paper":"B7"}]}"#),
            ErrorCode::ParseFailure
        );
    }

    #[test]
    fn an_unplaced_viewport_round_trips_with_its_empty_rectangle() {
        // A viewport not placed yet has Box2's empty value, made of infinities
        // JSON cannot write; it is the default, so it is left out, and read
        // back as the same. A null written by hand reads as it too.
        let mut set = SheetSet::default();
        set.sheets.push(named_sheet("UNPLACED"));
        assert!(set.sheets[0].viewports[0].rect.empty());
        let json = json_of(&set);
        assert!(!json.contains("rect"));
        assert!(read(&json) == set);
        let null = read(
            r#"{"format":"katana-sheets","version":1,"sheets":[{"viewports":[{"rect":null}]}]}"#,
        );
        assert!(null.sheets[0].viewports[0].rect == Box2::default());
    }

    #[test]
    fn only_what_differs_from_the_defaults_is_written() {
        // One sheet, one viewport, every member at its default but the ids:
        // the format, the version, the sheet's id and the viewport's id and
        // kind - keys in the order the JSON writer sorts them.
        let mut set = SheetSet::default();
        set.sheets.push(Sheet {
            id: text("s1"),
            viewports: vec![Viewport {
                id: text("vp1"),
                ..Viewport::default()
            }],
            ..Sheet::default()
        });
        assert_eq!(
            json_of(&set),
            r#"{"format":"katana-sheets","sheets":[{"id":"s1","viewports":[{"id":"vp1","kind":"plan"}]}],"version":1}"#
        );
        // A value equal to a default in every bit but the sign is not the
        // default: -0.0 is written, and reads back as -0.0.
        set.sheets[0].viewports[0].rotation = -0.0;
        let back = read(&json_of(&set));
        assert!(back.sheets[0].viewports[0].rotation.is_sign_negative());
        // Nothing at all: the format and the version.
        assert_eq!(
            json_of(&SheetSet::default()),
            r#"{"format":"katana-sheets","version":1}"#
        );
    }

    #[test]
    fn text_that_is_not_utf8_or_a_number_that_is_not_finite_cannot_be_stored() {
        // The C++ case begins with a name of bytes that are not UTF-8; a Rust
        // String cannot hold one (the model holds validated UTF-8, docs/rust.md
        // "Step 2"), so its twin is the half about numbers.
        let mut set = SheetSet::default();
        set.sheets.push(named_sheet("FINE"));
        set.sheets[0].viewports[0].centre.x = f64::NAN;
        assert_eq!(
            sheet_set_to_json(&set).expect_err("a NaN").code,
            ErrorCode::InvalidArgument
        );
        set.sheets[0].viewports[0].centre.x = 0.0;
        set.sheets[0].viewports[0].scale = f64::INFINITY;
        assert_eq!(
            sheet_set_to_json(&set).expect_err("an infinity").code,
            ErrorCode::InvalidArgument
        );
    }

    // ---- Document ------------------------------------------------------------------------

    #[test]
    fn setting_the_sheet_set_is_one_undoable_step_that_leaves_no_key_when_undone() {
        let mut document = Document::default();
        assert!(document.sheet_set().sheets.is_empty());
        assert!(!has_sheets_key(&document));
        let set = everything();
        ok(document.set_sheet_set(&set, Document::SET_SHEETS));
        assert!(*document.sheet_set() == set);
        assert!(has_sheets_key(&document));
        assert!(document.is_modified());
        ok(document.undo());
        assert!(document.sheet_set().sheets.is_empty());
        // The metadata is exactly as it was: no "sheets" key at all.
        assert!(!has_sheets_key(&document));
        assert!(!document.is_modified());
        ok(document.redo());
        assert!(*document.sheet_set() == set);
    }

    #[test]
    fn setting_the_same_set_again_records_no_step() {
        let mut document = Document::default();
        ok(document.set_sheet_set(&everything(), Document::SET_SHEETS));
        ok(document.set_sheet_set(&everything(), Document::SET_SHEETS));
        ok(document.undo());
        assert!(!document.history().can_undo()); // one step, not two
    }

    #[test]
    fn add_remove_move_and_duplicate_are_each_one_step() {
        let mut document = Document::default();
        ok(add_sheet(&mut document, named_sheet("A"), None));
        ok(add_sheet(&mut document, named_sheet("C"), None));
        ok(add_sheet(&mut document, named_sheet("B"), Some(1)));
        assert_eq!(names(&document), vec!["A", "B", "C"]);
        // Each sheet and viewport got an id of its own.
        assert_eq!(document.sheet_set().sheets[0].id, "s1");
        assert_eq!(document.sheet_set().sheets[1].id, "s3");
        assert_eq!(document.sheet_set().sheets[2].id, "s2");
        assert_eq!(document.sheet_set().sheets[1].viewports[0].id, "vp3");

        ok(move_sheet(&mut document, 0, 2));
        assert_eq!(names(&document), vec!["B", "C", "A"]);
        ok(duplicate_sheet(&mut document, 1));
        assert_eq!(names(&document), vec!["B", "C", "C (copy)", "A"]);
        let set = document.sheet_set();
        let copy = &set.sheets[2];
        assert_eq!(copy.id, "s4");
        assert_eq!(copy.viewports[0].id, "vp4");
        ok(remove_sheet(&mut document, 0));
        assert_eq!(names(&document), vec!["C", "C (copy)", "A"]);

        // Undo walks back one edit at a time.
        ok(document.undo());
        assert_eq!(names(&document), vec!["B", "C", "C (copy)", "A"]);
        ok(document.undo());
        assert_eq!(names(&document), vec!["B", "C", "A"]);
        ok(document.undo());
        assert_eq!(names(&document), vec!["A", "B", "C"]);
    }

    #[test]
    fn the_last_sheet_can_be_removed_and_an_empty_set_is_valid() {
        let mut document = Document::default();
        ok(add_sheet(&mut document, named_sheet("ONLY"), None));
        ok(remove_sheet(&mut document, 0));
        assert!(document.sheet_set().sheets.is_empty());
        assert!(document.sheet_set_status().is_ok());
        assert!(!has_sheets_key(&document)); // nothing left to store
        ok(document.undo());
        assert_eq!(names(&document), vec!["ONLY"]);
    }

    #[test]
    fn an_edit_out_of_range_or_refused_changes_nothing() {
        let mut document = Document::default();
        ok(add_sheet(&mut document, named_sheet("A"), None));
        let before = SheetSet::clone(&document.sheet_set());
        assert_eq!(
            remove_sheet(&mut document, 1).expect_err("no sheet 1").code,
            ErrorCode::NotFound
        );
        assert_eq!(
            move_sheet(&mut document, 0, 1)
                .expect_err("no sheet 1")
                .code,
            ErrorCode::NotFound
        );
        assert_eq!(
            duplicate_sheet(&mut document, 3)
                .expect_err("no sheet 3")
                .code,
            ErrorCode::NotFound
        );
        assert_eq!(
            add_sheet(&mut document, named_sheet("B"), Some(5))
                .expect_err("no position 5")
                .code,
            ErrorCode::NotFound
        );
        assert_eq!(
            edit_viewport(&mut document, "vp99", |_| Ok(()), EDIT_VIEWPORT)
                .expect_err("no viewport vp99")
                .code,
            ErrorCode::NotFound
        );
        let refused: Status = edit_viewport(
            &mut document,
            "vp1",
            |viewport| {
                viewport.scale = 1.0; // changed, then refused: the change must not stick
                Err(make_error(ErrorCode::InvalidArgument, "no", ""))
            },
            EDIT_VIEWPORT,
        );
        assert_eq!(
            refused.expect_err("refused").code,
            ErrorCode::InvalidArgument
        );
        assert!(*document.sheet_set() == before);
        // Only the one add is on the history.
        ok(document.undo());
        assert!(!document.history().can_undo());
    }

    #[test]
    fn editing_a_viewport_finds_it_by_id_on_any_sheet() {
        let mut document = Document::default();
        ok(add_sheet(&mut document, named_sheet("A"), None));
        ok(add_sheet(&mut document, named_sheet("B"), None));
        ok(edit_viewport(
            &mut document,
            "vp2",
            |viewport| {
                viewport.scale = 250.0;
                viewport.hidden_layers.hide("TREES");
                Ok(())
            },
            EDIT_VIEWPORT,
        ));
        assert_eq!(document.sheet_set().sheets[1].viewports[0].scale, 250.0);
        assert!(
            document.sheet_set().sheets[1].viewports[0]
                .hidden_layers
                .hides("TREES")
        );
        assert_eq!(document.sheet_set().sheets[0].viewports[0].scale, 500.0);
        ok(document.undo());
        assert_eq!(document.sheet_set().sheets[1].viewports[0].scale, 500.0);
    }

    #[test]
    fn generated_sheets_join_a_set_in_one_step_with_their_references_intact() {
        let mut document = Document::default();
        ok(add_sheet(&mut document, named_sheet("COVER"), None));
        let grid = GridRequest {
            area: Box2::new(Point2::new(0.0, 0.0), Point2::new(400.0, 200.0)),
            scale: 500.0,
            ..GridRequest::default()
        };
        let tiles = ok(grid_sheets(&grid, None));
        ok(add_sheets(&mut document, tiles, ADD_SHEETS));
        let set = document.sheet_set();
        assert_eq!(set.sheets.len(), 8); // the cover, a key plan, six tiles
        // The key plan's first outline is the first tile, now sheet 3.
        assert_eq!(mark_label(&set, &set.sheets[1].viewports[0].marks[0]), "3");
        ok(document.undo());
        assert_eq!(document.sheet_set().sheets.len(), 1);
    }

    #[test]
    fn a_sheet_added_after_the_last_tile_is_removed_is_not_led_to_by_the_tiles_marks() {
        // 400 x 100 m at 1 : 500: tiles of 192.5 x 125 m, three across and one
        // down (400 / 192.5 = 2.08), after a key plan - s1 the key plan, s2 to
        // s4 the tiles. The middle tile's match lines lead right to s4 and
        // left to s2; the key plan's third outline is s4.
        let mut document = Document::default();
        let grid = GridRequest {
            area: Box2::new(Point2::new(0.0, 0.0), Point2::new(400.0, 100.0)),
            scale: 500.0,
            ..GridRequest::default()
        };
        let tiles = ok(grid_sheets(&grid, None));
        ok(add_sheets(&mut document, tiles, ADD_SHEETS));
        assert_eq!(document.sheet_set().sheets.len(), 4);
        let to_last = document.sheet_set().sheets[2].viewports[0].marks[0].clone();
        assert_eq!(to_last.sheet, "s4");
        assert_eq!(
            mark_label(&document.sheet_set(), &to_last),
            "MATCH LINE - SEE SHEET 4"
        );

        // The last tile goes and a legend comes. The legend is sheet 4 now,
        // but not s4: the match line and the outline that led to the removed
        // tile lead nowhere, rather than to the legend.
        ok(remove_sheet(&mut document, 3));
        ok(add_sheet(&mut document, named_sheet("LEGEND"), None));
        let set = document.sheet_set();
        assert_eq!(set.sheets.len(), 4);
        assert_eq!(set.sheets[3].id, "s5");
        assert_eq!(
            mark_label(&set, &set.sheets[2].viewports[0].marks[0]),
            "MATCH LINE"
        );
        assert_eq!(mark_label(&set, &set.sheets[0].viewports[0].marks[2]), "");
        // The first tile's outline still reads 2.
        assert_eq!(mark_label(&set, &set.sheets[0].viewports[0].marks[0]), "2");
    }

    #[test]
    fn sheets_written_by_a_newer_version_are_kept_and_not_overwritten() {
        let mut document = Document::default();
        let mut metadata = document.metadata().clone();
        let newer = r#"{"format":"katana-sheets","version":99}"#;
        metadata.unknown_keys.insert(text("sheets"), text(newer));
        document.set_metadata(metadata);
        assert!(document.sheet_set().sheets.is_empty());
        assert_eq!(
            document
                .sheet_set_status()
                .expect_err("a newer version")
                .code,
            ErrorCode::Unsupported
        );
        assert_eq!(
            add_sheet(&mut document, named_sheet("A"), None)
                .expect_err("kept as it is")
                .code,
            ErrorCode::CommandRejected
        );
        assert_eq!(
            document
                .metadata()
                .unknown_keys
                .get("sheets")
                .map(String::as_str),
            Some(newer)
        );
    }

    #[test]
    fn the_sheet_set_is_read_again_when_the_metadata_is_replaced() {
        let mut document = Document::default();
        ok(add_sheet(&mut document, named_sheet("A"), None));
        assert_eq!(document.sheet_set().sheets.len(), 1);
        let mut metadata = document.metadata().clone();
        metadata.unknown_keys.remove("sheets");
        document.set_metadata(metadata);
        assert!(document.sheet_set().sheets.is_empty());
    }

    #[test]
    fn sheets_survive_a_save_and_reopen_with_no_schema_change() {
        let scratch = ScratchDirectory::new("sheets-save");
        let project = scratch.path.join("sheets.katana");
        let set = everything();
        {
            let mut document = Document::default();
            ok(document.set_sheet_set(&set, Document::SET_SHEETS));
            ok(document.save_as(&project));
            assert!(!document.is_modified());
        }
        let mut reopened = Document::default();
        ok(reopened.open(&project));
        assert!(reopened.sheet_set_status().is_ok());
        assert!(*reopened.sheet_set() == set);
        assert!(!reopened.is_modified());
    }

    // ---- The logo ------------------------------------------------------------------------

    #[test]
    fn a_logo_is_copied_into_the_projects_assets_and_its_name_is_one_undoable_step() {
        let scratch = ScratchDirectory::new("sheets-logo");
        let project = scratch.path.join("logo.katana");
        let source = scratch.path.join("Our Logo!.png");
        write_file(&source, &png_bytes());
        let mut document = Document::default();
        // Nowhere to keep it until the drawing is a project.
        assert_eq!(
            import_logo(&mut document, &source)
                .expect_err("not a project")
                .code,
            ErrorCode::InvalidState
        );
        ok(document.save_as(&project));

        let name = ok(import_logo(&mut document, &source));
        // The stem made safe for any file system; the extension from the
        // content.
        assert_eq!(name, "Our_Logo_.png");
        assert_eq!(document.sheet_set().defaults.logo_asset, "Our_Logo_.png");
        let path = logo_path(&document).expect("a logo on disk");
        assert_eq!(path, project.join("assets").join("Our_Logo_.png"));
        assert_eq!(
            std::fs::metadata(path).map(|found| found.len()).ok(),
            Some(png_bytes().len() as u64)
        );

        // The same image again is found, not copied twice; a different image
        // of the same name gets a name of its own rather than replacing it.
        assert_eq!(ok(import_logo(&mut document, &source)), "Our_Logo_.png");
        let mut changed = png_bytes();
        changed.extend_from_slice(b"changed");
        write_file(&source, &changed);
        assert_eq!(ok(import_logo(&mut document, &source)), "Our_Logo_-2.png");
        assert!(project.join("assets").join("Our_Logo_.png").exists());

        // Undo takes the name back; the file stays for a redo.
        ok(document.undo());
        assert_eq!(document.sheet_set().defaults.logo_asset, "Our_Logo_.png");
        ok(document.undo());
        assert!(document.sheet_set().defaults.logo_asset.is_empty());
        assert!(logo_path(&document).is_none());
        assert!(project.join("assets").join("Our_Logo_-2.png").exists());
    }

    #[test]
    fn a_logo_must_be_a_small_image_of_a_known_kind() {
        let scratch = ScratchDirectory::new("sheets-logo-kinds");
        let project = scratch.path.join("kinds.katana");
        let mut document = Document::default();
        ok(document.save_as(&project));
        // Recognised by content whatever the extension says.
        write_file(&scratch.path.join("a.dat"), b"\xff\xd8\xff\xe0 jpeg");
        assert_eq!(
            ok(import_logo(&mut document, &scratch.path.join("a.dat"))),
            "a.jpg"
        );
        write_file(&scratch.path.join("b.img"), b"GIF89a....");
        assert_eq!(
            ok(import_logo(&mut document, &scratch.path.join("b.img"))),
            "b.gif"
        );
        write_file(&scratch.path.join("c.img"), b"BM......");
        assert_eq!(
            ok(import_logo(&mut document, &scratch.path.join("c.img"))),
            "c.bmp"
        );
        // Not an image.
        write_file(&scratch.path.join("d.png"), b"<svg/>");
        assert_eq!(
            import_logo(&mut document, &scratch.path.join("d.png"))
                .expect_err("not an image")
                .code,
            ErrorCode::Unsupported
        );
        // Over the cap: 4 MiB and one byte.
        let large = scratch.path.join("large.png");
        write_file(&large, &png_bytes());
        std::fs::File::options()
            .write(true)
            .open(&large)
            .and_then(|file| file.set_len(MAXIMUM_LOGO_BYTES + 1))
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            import_logo(&mut document, &large)
                .expect_err("too large")
                .code,
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            import_logo(&mut document, &scratch.path.join("missing.png"))
                .expect_err("not there")
                .code,
            ErrorCode::NotFound
        );
        assert_eq!(document.sheet_set().defaults.logo_asset, "c.bmp"); // the failures changed nothing
    }

    // ---- Field context -------------------------------------------------------------------

    #[test]
    fn the_project_supplies_the_field_context_and_dates_print_day_month_year() {
        let scratch = ScratchDirectory::new("sheets-context");
        let mut document = Document::default();
        let mut metadata = document.metadata().clone();
        metadata.description = text("Detail survey");
        metadata.coordinate_system = text("EPSG:7856");
        document.set_metadata(metadata);
        ok(document.save_as(&scratch.path.join("main_road.katana")));
        let context = field_context_for(&document, text("24/09/26"));
        assert_eq!(context.project_name, "main_road"); // a project is named after its directory
        assert_eq!(context.project_description, "Detail survey");
        assert_eq!(context.coordinate_system, "EPSG:7856");
        assert_eq!(context.file_name, "main_road.katana");
        assert_eq!(context.plot_date, "24/09/26");
        assert_eq!(frame_date(YearMonthDay::new(2026, 9, 24)), "24/09/26");
        assert_eq!(frame_date(YearMonthDay::new(2031, 1, 5)), "05/01/31");
    }
}
