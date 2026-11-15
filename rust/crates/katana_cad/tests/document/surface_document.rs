// The drawing's surfaces and their display styles in the Document (document.rs,
// "surfaces"; docs/terrain.md, "Surfaces in the drawing";
// tests/cad/terrain/test_surface_document.cpp): one store, every change one
// undo step that puts back the exact store, a generation that only rises,
// cleared by New, saved and opened with the project.
//
// Two C++ cases build style text that is not UTF-8 (the bytes a katana_cli on
// Windows hands over), which no String holds: they keep their UTF-8 half. The
// two whose bad bytes come out of a project's database are twinned whole: a
// surface record's text is bytes until the Document reads it.

use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::surface_test_support::{
    ScratchDirectory, flat_square, tilted_square, with, with_details,
};
use katana_cad::terrain::surface_document_io::surface_styles_metadata_value;
use katana_cad::terrain::surface_style::{built_in_surface_styles, surface_styles_to_json};
use katana_cad::{Document, DocumentChange, ListenerHandle, SurfaceDisplayStyle, SurfaceStyleSet};
use katana_commands::{EntityAttributes, Transaction, create_point};
use katana_core::ErrorCode;
use katana_geometry::Point2;
use katana_storage::{OpenMode, SqliteDatabase};
use katana_terrain::{SurfaceStore, TIN_CODEC_VERSION, encode_tin};

struct Recorder {
    parts: Arc<Mutex<Vec<u32>>>,
    _handle: ListenerHandle,
}

impl Recorder {
    fn new(document: &Document) -> Self {
        let parts = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&parts);
        let handle = document.add_change_listener(move |change| {
            sink.lock().expect("the recorder").push(change.parts);
        });
        Self {
            parts,
            _handle: handle,
        }
    }

    fn parts(&self) -> Vec<u32> {
        self.parts.lock().expect("the recorder").clone()
    }

    fn last(&self) -> DocumentChange<'static> {
        DocumentChange::of(*self.parts().last().expect("a notification"))
    }
}

/// Every surface's name, in order.
fn names(document: &Document) -> Vec<String> {
    document
        .surfaces()
        .all()
        .iter()
        .map(|named| named.name.clone())
        .collect()
}

/// A copy of the built-ins with one style of the test's own.
fn with_style(name: &str) -> SurfaceStyleSet {
    let mut set = built_in_surface_styles();
    let mut style = SurfaceDisplayStyle {
        name: String::from(name),
        show_triangles: true,
        ..SurfaceDisplayStyle::default()
    };
    style.contours.interval = 0.25;
    set.styles.push(style);
    set
}

fn empty() -> SurfaceStore {
    SurfaceStore::default()
}

fn database(project: &Path) -> SqliteDatabase {
    SqliteDatabase::open(&project.join("project.db"), OpenMode::ReadWriteCreate)
        .unwrap_or_else(|error| panic!("{}", error.describe()))
}

/// The text of the metadata row `surface_styles` as a saved project holds it.
fn stored_styles(project: &Path) -> Vec<u8> {
    let raw = database(project);
    let mut select = raw
        .prepare("SELECT value FROM metadata WHERE key = 'surface_styles'")
        .unwrap_or_else(|error| panic!("{}", error.describe()));
    assert!(select.step().unwrap_or(false));
    select.column_text(0)
}

fn must(status: katana_core::Status) {
    if let Err(error) = status {
        panic!("{}", error.describe());
    }
}

mod surface_document {
    use super::*;

    #[test]
    fn a_new_document_has_no_surfaces_and_the_built_in_styles() {
        let document = Document::default();
        assert!(document.surfaces().empty());
        assert!(*document.surface_styles() == built_in_surface_styles());
        assert!(document.surface_styles_status().is_ok());
    }

    #[test]
    fn setting_the_surfaces_is_one_undo_step_and_undo_puts_back_the_exact_store() {
        let mut document = Document::default();
        let ground = tilted_square(100.0, 1.0, 10.0, 0.0, 0.0);
        must(document.set_surfaces(
            with(empty(), "ground", Arc::clone(&ground), ""),
            "SURFACE FROM",
        ));
        assert_eq!(document.history().undo_count(), 1);
        assert_eq!(document.history().undo_name(), "SURFACE FROM");
        assert!(document.is_modified());
        let one = document.surfaces().clone();

        must(document.set_surfaces(
            with(one.clone(), "design", flat_square(101.0), ""),
            "SURFACE COPY",
        ));
        assert_eq!(names(&document), vec!["ground", "design"]);

        must(document.undo());
        // The very surface, by its pointer.
        assert_eq!(document.surfaces().all(), one.all());
        let found = document.surfaces().find("ground").expect("ground");
        assert!(Arc::ptr_eq(
            found.surface.as_ref().expect("a surface"),
            &ground
        ));
        must(document.undo());
        assert!(document.surfaces().empty());
        must(document.redo());
        assert_eq!(document.surfaces().all(), one.all());
    }

    #[test]
    fn the_generation_rises_on_every_change_undo_and_redo_included_and_never_falls() {
        let mut document = Document::default();
        let mut seen = vec![document.surfaces_generation()];
        must(document.set_surfaces(
            with(empty(), "ground", flat_square(0.0), ""),
            Document::SET_SURFACES,
        ));
        seen.push(document.surfaces_generation());
        let mut hidden = document.surfaces().clone();
        must(hidden.set_visible("ground", false));
        must(document.set_surfaces(hidden, Document::SET_SURFACES));
        seen.push(document.surfaces_generation());
        must(document.undo());
        seen.push(document.surfaces_generation());
        must(document.undo());
        seen.push(document.surfaces_generation());
        must(document.redo());
        seen.push(document.surfaces_generation());
        document.new_document();
        seen.push(document.surfaces_generation());
        for (i, pair) in seen.windows(2).enumerate() {
            assert!(pair[1] > pair[0], "change {}", i + 1);
        }
    }

    #[test]
    fn a_store_equal_to_the_drawings_records_nothing() {
        let mut document = Document::default();
        must(document.set_surfaces(
            with(empty(), "ground", flat_square(0.0), ""),
            Document::SET_SURFACES,
        ));
        let generation = document.surfaces_generation();
        // A copy changed and changed back is still the same surfaces, though
        // its revision has moved twice.
        let mut same = document.surfaces().clone();
        must(same.set_visible("ground", false));
        must(same.set_visible("ground", true));
        must(document.set_surfaces(same, Document::SET_SURFACES));
        assert_eq!(document.history().undo_count(), 1);
        assert_eq!(document.surfaces_generation(), generation);
        let command = document
            .surfaces_command(document.surfaces().clone(), Document::SET_SURFACES)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(command.is_none());
    }

    #[test]
    fn a_surface_in_a_style_the_drawing_does_not_have_is_refused_and_nothing_changes() {
        let mut document = Document::default();
        let refused = document
            .set_surfaces(
                with(empty(), "ground", flat_square(0.0), "No Such"),
                Document::SET_SURFACES,
            )
            .expect_err("no such style");
        assert_eq!(refused.code, ErrorCode::InvalidArgument);
        assert!(document.surfaces().empty());
        assert_eq!(document.history().undo_count(), 0);
        // Named in any case, as a line names it.
        must(document.set_surfaces(
            with(empty(), "ground", flat_square(0.0), "contours"),
            Document::SET_SURFACES,
        ));
    }

    #[test]
    fn a_style_a_surface_is_drawn_in_cannot_be_taken_away() {
        let mut document = Document::default();
        must(document.set_surface_styles(with_style("Fine"), Document::SET_SURFACE_STYLES));
        must(document.set_surfaces(
            with(empty(), "ground", flat_square(0.0), "Fine"),
            Document::SET_SURFACES,
        ));
        let refused = document
            .set_surface_styles(built_in_surface_styles(), Document::SET_SURFACE_STYLES)
            .expect_err("a style in use");
        assert_eq!(refused.code, ErrorCode::InvalidArgument);
        assert!(refused.message.contains("ground"), "{}", refused.describe());
        assert!(document.surface_styles().find("Fine").is_some());
    }

    #[test]
    fn a_style_set_that_cannot_be_drawn_is_refused() {
        let mut document = Document::default();
        let mut broken = built_in_surface_styles();
        broken.styles[0].contours.interval = 0.0;
        assert_eq!(
            document
                .set_surface_styles(broken, Document::SET_SURFACE_STYLES)
                .expect_err("broken")
                .code,
            ErrorCode::InvalidArgument
        );
        let mut no_default = built_in_surface_styles();
        no_default.default_style = String::from("Nothing");
        assert_eq!(
            document
                .set_surface_styles(no_default, Document::SET_SURFACE_STYLES)
                .expect_err("no default")
                .code,
            ErrorCode::InvalidArgument
        );
        assert!(*document.surface_styles() == built_in_surface_styles());
        assert_eq!(document.history().undo_count(), 0);
    }

    #[test]
    fn setting_the_styles_is_one_undo_step_that_puts_back_the_exact_set() {
        let mut document = Document::default();
        let generation = document.surface_styles_generation();
        must(document.set_surface_styles(with_style("Fine"), "SURFACESTYLE NEW"));
        assert_eq!(document.history().undo_name(), "SURFACESTYLE NEW");
        assert!(document.surface_styles_generation() > generation);
        must(document.undo());
        assert!(*document.surface_styles() == built_in_surface_styles());
        must(document.redo());
        assert!(*document.surface_styles() == with_style("Fine"));
        // The same set again is no step.
        must(document.set_surface_styles(with_style("Fine"), Document::SET_SURFACE_STYLES));
        assert_eq!(document.history().undo_count(), 1);
    }

    #[test]
    fn a_renamed_style_and_the_surfaces_drawn_in_it_move_in_one_step() {
        let mut document = Document::default();
        must(document.set_surface_styles(with_style("Fine"), Document::SET_SURFACE_STYLES));
        must(document.set_surfaces(
            with(empty(), "ground", flat_square(0.0), "Fine"),
            Document::SET_SURFACES,
        ));
        let renamed = with_style("Finer");
        let mut moved = document.surfaces().clone();
        must(moved.set_style("ground", "Finer"));
        let steps = document.history().undo_count();
        must(document.set_surfaces_and_styles(moved, renamed, "SURFACESTYLE RENAME"));
        assert_eq!(document.history().undo_count(), steps + 1);
        assert_eq!(
            document.surfaces().find("ground").expect("ground").style,
            "Finer"
        );
        assert!(document.surface_styles().find("Finer").is_some());
        must(document.undo());
        assert_eq!(
            document.surfaces().find("ground").expect("ground").style,
            "Fine"
        );
        assert!(document.surface_styles().find("Fine").is_some());
    }

    #[test]
    fn every_change_is_notified_as_the_drawings_surfaces_or_styles() {
        let mut document = Document::default();
        let recorder = Recorder::new(&document);
        must(document.set_surfaces(
            with(empty(), "ground", flat_square(0.0), ""),
            Document::SET_SURFACES,
        ));
        assert_eq!(recorder.parts().len(), 1);
        assert_eq!(
            recorder.parts().last().copied(),
            Some(DocumentChange::SURFACES | DocumentChange::HISTORY)
        );
        assert!(recorder.last().changes_drawing());
        must(document.undo());
        assert!(recorder.last().has(DocumentChange::SURFACES));
        must(document.redo());
        assert!(recorder.last().has(DocumentChange::SURFACES));
        must(document.set_surface_styles(with_style("Fine"), Document::SET_SURFACE_STYLES));
        assert_eq!(
            recorder.parts().last().copied(),
            Some(DocumentChange::SURFACE_STYLES | DocumentChange::HISTORY)
        );
        document.new_document();
        assert!(recorder.last().has(DocumentChange::REPLACED));
        assert!(recorder.last().has(DocumentChange::SURFACES));
        assert!(recorder.last().has(DocumentChange::SURFACE_STYLES));
    }

    #[test]
    fn new_clears_the_surfaces_and_puts_back_the_built_in_styles() {
        let mut document = Document::default();
        must(document.set_surface_styles(with_style("Fine"), Document::SET_SURFACE_STYLES));
        must(document.set_surfaces(
            with(empty(), "ground", flat_square(0.0), "Fine"),
            Document::SET_SURFACES,
        ));
        document.new_document();
        assert!(document.surfaces().empty());
        assert!(*document.surface_styles() == built_in_surface_styles());
        assert!(!document.history().can_undo());
    }

    #[test]
    fn surfaces_and_styles_survive_a_save_and_an_open_exactly() {
        let scratch = ScratchDirectory::new("save-open");
        let project = scratch.path.join("surfaces.katana");
        let ground = tilted_square(100.125, 0.1, 10.0, 0.0, 0.0);
        let design = tilted_square(98.0, -2.0, 5.0, 2.5, 2.5);
        {
            let mut document = Document::default();
            must(document.set_surface_styles(with_style("Fine"), Document::SET_SURFACE_STYLES));
            let store = with_details(
                empty(),
                "ground",
                Arc::clone(&ground),
                "",
                true,
                "point cloud site.laz",
            );
            let store = with_details(
                store,
                "Design Pad",
                Arc::clone(&design),
                "Fine",
                false,
                "drawing (4 entities)",
            );
            must(document.set_surfaces(store, Document::SET_SURFACES));
            must(document.save_as(&project));
            assert!(!document.is_modified());
        }
        let mut reopened = Document::default();
        must(reopened.open(&project));
        assert!(!reopened.is_modified());
        assert_eq!(names(&reopened), vec!["ground", "Design Pad"]);
        let first = &reopened.surfaces().all()[0];
        let second = &reopened.surfaces().all()[1];
        // Every vertex, triangle and flag.
        assert!(**first.surface.as_ref().expect("a surface") == *ground);
        assert!(**second.surface.as_ref().expect("a surface") == *design);
        assert_eq!(first.source, "point cloud site.laz");
        assert_eq!(first.style, "");
        assert!(first.visible);
        assert_eq!(second.style, "Fine");
        assert!(!second.visible);
        assert!(*reopened.surface_styles() == with_style("Fine"));
        assert!(reopened.surface_styles_status().is_ok());
        // The styles' key is the Document's, written at the save: not a key
        // the metadata carries for someone else.
        assert!(
            !reopened
                .metadata()
                .unknown_keys
                .contains_key("surface_styles")
        );
    }

    #[test]
    fn opening_a_project_with_none_leaves_none_and_the_built_ins() {
        let scratch = ScratchDirectory::new("open-none");
        let project = scratch.path.join("plain.katana");
        {
            let mut document = Document::default();
            must(document.save_as(&project));
        }
        let mut reopened = Document::default();
        must(reopened.set_surfaces(
            with(empty(), "left over", flat_square(0.0), ""),
            Document::SET_SURFACES,
        ));
        must(reopened.open(&project));
        assert!(
            reopened.surfaces().empty(),
            "the open replaces the drawing's surfaces"
        );
        assert!(*reopened.surface_styles() == built_in_surface_styles());
    }

    #[test]
    fn styles_a_newer_katana_wrote_are_kept_and_not_overwritten() {
        let scratch = ScratchDirectory::new("newer-styles");
        let project = scratch.path.join("newer.katana");
        {
            let mut document = Document::default();
            must(document.save_as(&project));
        }
        let newer = r#"{"format":"katana-surface-styles","version":99,"styles":[]}"#;
        {
            let raw = database(&project);
            must(raw.execute(&format!(
                "UPDATE metadata SET value = '{newer}' WHERE key = 'surface_styles'"
            )));
        }
        let mut document = Document::default();
        must(document.open(&project));
        assert_eq!(
            document
                .surface_styles_status()
                .expect_err("a newer set")
                .code,
            ErrorCode::Unsupported
        );
        assert!(
            *document.surface_styles() == built_in_surface_styles(),
            "the built-ins stand in"
        );
        let refused = document
            .set_surface_styles(with_style("Fine"), Document::SET_SURFACE_STYLES)
            .expect_err("a newer set is kept");
        assert_eq!(refused.code, ErrorCode::CommandRejected);
        // A surface is still made, in the styles there are.
        must(document.set_surfaces(
            with(empty(), "ground", flat_square(0.0), "Contours"),
            Document::SET_SURFACES,
        ));
        must(document.save());
        assert_eq!(
            stored_styles(&project),
            newer.as_bytes(),
            "written back as it was"
        );
    }

    #[test]
    fn a_surface_the_project_cannot_read_refuses_the_open_and_leaves_the_drawing() {
        let scratch = ScratchDirectory::new("newer-surface");
        let project = scratch.path.join("newer.katana");
        {
            let mut document = Document::default();
            must(document.set_surfaces(
                with(empty(), "ground", flat_square(0.0), ""),
                Document::SET_SURFACES,
            ));
            must(document.save_as(&project));
        }
        {
            // The codec's version word (bytes 4 to 7) made one newer than this
            // build writes.
            let mut bytes = encode_tin(&flat_square(0.0));
            bytes[4] = (TIN_CODEC_VERSION + 1) as u8;
            let raw = database(&project);
            let mut update = raw
                .prepare("UPDATE surfaces SET data = ?1")
                .unwrap_or_else(|error| panic!("{}", error.describe()));
            must(update.bind(1, bytes.as_slice()));
            must(update.run());
        }
        let mut document = Document::default();
        must(document.set_surfaces(
            with(empty(), "mine", flat_square(5.0), ""),
            Document::SET_SURFACES,
        ));
        let opened = document.open(&project).expect_err("a newer surface");
        assert_eq!(opened.code, ErrorCode::Unsupported);
        assert!(opened.message.contains("ground"), "{}", opened.describe());
        assert_eq!(names(&document), vec!["mine"], "the drawing as it was");
    }

    #[test]
    fn an_import_of_entities_and_surfaces_is_one_step_through_the_command() {
        let mut document = Document::default();
        // Made through a shared Document, as a tool holding one makes it:
        // making the command changes nothing.
        let seen: &Document = &document;
        let command = seen
            .surfaces_command(with(empty(), "ground", flat_square(0.0), ""), "IMPORT")
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(document.surfaces().empty());
        let command = command.expect("a change to make");
        let mut transaction = Transaction::new("IMPORT site.12da");
        transaction.add(create_point(
            Point2::new(1.0, 2.0),
            EntityAttributes::default(),
        ));
        transaction.add(command);
        must(document.execute(Box::new(transaction)));
        assert_eq!(document.history().undo_count(), 1);
        assert_eq!(document.surfaces().all().len(), 1);
        assert_eq!(document.model().entities.size(), 1);
        must(document.undo());
        assert!(document.surfaces().empty());
        assert_eq!(document.model().entities.size(), 0);
    }

    // ---- text that is not UTF-8, and a style set that is not the form -------------------

    #[test]
    fn a_style_set_whose_text_is_not_utf8_is_refused_so_a_save_never_meets_it() {
        // The C++ hands the door "Pente" with 0xB0 and "caf" with 0xE9, the
        // ANSI code page's degree sign and e-acute, which no String holds. The
        // UTF-8 half: the same letters as UTF-8 are accepted, each as one step,
        // and the save that meets them is a Status.
        let scratch = ScratchDirectory::new("ansi-styles");
        let mut document = Document::default();
        must(document.set_surfaces(
            with(empty(), "ground", flat_square(0.0), ""),
            Document::SET_SURFACES,
        ));

        must(document.set_surface_styles(with_style("Pente\u{B0}"), Document::SET_SURFACE_STYLES));
        let mut described = with_style("Fine");
        if let Some(style) = described.styles.last_mut() {
            style.description = String::from("caf\u{E9}");
        }
        must(document.set_surface_styles(described.clone(), Document::SET_SURFACE_STYLES));

        assert!(*document.surface_styles() == described);
        assert_eq!(document.history().undo_count(), 3);
        must(document.save_as(&scratch.path.join("p.katana")));
    }

    #[test]
    fn a_surface_name_an_earlier_build_stored_as_ansi_bytes_or_with_a_quote_is_repaired_on_open() {
        // Projects the build before this one could write: a name typed at a
        // katana_cli on Windows (0xE9 for e-acute) and one with a double quote
        // that an importer let through. Opening must not fail for them - the
        // person could not rename what a line cannot name - and must not leave
        // a name the JSON writers fail on.
        let scratch = ScratchDirectory::new("repair-names");
        let project = scratch.path.join("old.katana");
        {
            let mut document = Document::default();
            let store = with(empty(), "ground", flat_square(0.0), "");
            let store = with(store, "other", flat_square(1.0), "");
            let store = with(store, "clean", flat_square(2.0), "");
            must(document.set_surfaces(store, Document::SET_SURFACES));
            must(document.save_as(&project));
        }
        {
            let raw = database(&project);
            // X'636166E9' is the four bytes of "caf" and 0xE9, stored as text
            // exactly as SQLite is given them (it does not check).
            must(raw.execute(
                "UPDATE surfaces SET name = CAST(X'636166E9' AS TEXT) WHERE name = 'ground'",
            ));
            must(raw.execute("UPDATE surfaces SET name = 'PAD \"A\"' WHERE name = 'other'"));
        }
        let mut document = Document::default();
        must(document.open(&project));
        // 0xE9 read as Windows-1252 is U+00E9, UTF-8 C3 A9; the quote an
        // apostrophe.
        assert_eq!(names(&document), vec!["caf\u{E9}", "PAD 'A'", "clean"]);
        // And the repaired project is stable: saved and opened again, nothing
        // moves.
        must(document.save());
        let mut again = Document::default();
        must(again.open(&project));
        assert_eq!(names(&again), names(&document));
    }

    #[test]
    fn a_surface_source_and_style_an_earlier_build_stored_as_ansi_bytes_are_read_as_windows1252() {
        // Free text a katana_cli on Windows typed in the ANSI code page: a
        // source "raster caf" + 0xE9 (e-acute) and a style named "Pente" +
        // 0xB0 (the degree sign). Neither is UTF-8, and the project's own JSON
        // replies and its next save fail on such bytes, so open reads them as
        // the Windows-1252 they were: U+00E9 is C3 A9, U+00B0 is C2 B0.
        let scratch = ScratchDirectory::new("repair-text");
        let project = scratch.path.join("old.katana");
        {
            let mut document = Document::default();
            must(document.set_surfaces(
                with(empty(), "ground", flat_square(0.0), ""),
                Document::SET_SURFACES,
            ));
            must(document.save_as(&project));
        }
        {
            let raw = database(&project);
            // X'72617374657220636166E9' is "raster caf" and 0xE9;
            // X'50656E7465B0' is "Pente" and 0xB0, stored as text exactly as
            // SQLite is given them.
            must(raw.execute(
                "UPDATE surfaces SET source = CAST(X'72617374657220636166E9' AS TEXT), \
                 style = CAST(X'50656E7465B0' AS TEXT) WHERE name = 'ground'",
            ));
        }
        let mut document = Document::default();
        must(document.open(&project));
        let ground = document.surfaces().find("ground").expect("ground").clone();
        assert_eq!(ground.source, "raster caf\u{E9}");
        assert_eq!(ground.style, "Pente\u{B0}");
        // The project is stable: saved and opened again, nothing moves,
        // nothing fails.
        must(document.save());
        let mut again = Document::default();
        must(again.open(&project));
        let reread = again.surfaces().find("ground").expect("ground");
        assert_eq!(reread.source, ground.source);
        assert_eq!(reread.style, ground.style);
    }

    #[test]
    fn the_save_refuses_a_style_set_the_json_writer_could_not_carry_rather_than_rewrite_its_text() {
        // The Document's set_surface_styles refuses such a set on the way in,
        // and the guard in the save is a function of the set, handed the sets
        // the door refuses. The C++ hands it text that is not UTF-8, which no
        // String holds; the rest of the case is twinned: a set the check
        // refuses for another reason is refused as well, and what it lets
        // through is nothing for the built-ins and the set's JSON for another.
        let mut twice = with_style("Fine");
        let last = twice.styles[twice.styles.len() - 1].clone();
        twice.styles.push(last);
        assert!(surface_styles_metadata_value(&twice).is_err());

        let built_in = surface_styles_metadata_value(&built_in_surface_styles())
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(built_in, "");
        let custom = surface_styles_metadata_value(&with_style("Fine"))
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(custom, surface_styles_to_json(&with_style("Fine")));
        assert!(custom.contains("Fine"));
    }

    #[test]
    fn a_style_set_whose_format_is_not_a_string_leaves_the_built_ins_in_place_and_the_text_kept() {
        // A "format" of a number, null, an object or an array made nlohmann's
        // value() throw type_error.302 out of Document::open (a process exit
        // in katana_cli and in the window). The documented outcome is the
        // built-ins standing in with a ParseFailure status and the damaged
        // text kept as it was.
        let scratch = ScratchDirectory::new("format-type");
        let project = scratch.path.join("damaged.katana");
        {
            let mut document = Document::default();
            must(document.save_as(&project));
        }
        for damaged in [
            r#"{"format":5,"version":1,"styles":[]}"#,
            r#"{"format":null,"version":1,"styles":[]}"#,
            r#"{"format":{},"version":1,"styles":[]}"#,
            r#"{"format":["katana-surface-styles"],"version":1}"#,
            r#"{"format":true,"version":1}"#,
            r#"[1,2,3]"#,
            r#""katana-surface-styles""#,
        ] {
            {
                let raw = database(&project);
                must(raw.execute(&format!(
                    "UPDATE metadata SET value = '{damaged}' WHERE key = 'surface_styles'"
                )));
            }
            let mut document = Document::default();
            let opened = document.open(&project);
            assert!(opened.is_ok(), "{damaged}: {opened:?}");
            let status = document.surface_styles_status();
            assert_eq!(
                status.expect_err(damaged).code,
                ErrorCode::ParseFailure,
                "{damaged}"
            );
            assert!(
                *document.surface_styles() == built_in_surface_styles(),
                "{damaged}"
            );
            must(document.save());
            assert_eq!(
                stored_styles(&project),
                damaged.as_bytes(),
                "a set that cannot be read is written back as it was"
            );
        }
    }
}
