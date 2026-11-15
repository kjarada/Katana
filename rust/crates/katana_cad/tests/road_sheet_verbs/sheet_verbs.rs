// The sheet verbs (tests/cad/plotting/test_sheet_verbs.cpp;
// plotting/sheet_verbs.rs): the sheet system driven from the text command
// line, as a person, a script, katana_cli and an AI agent drive it.
//
// Every edit is ONE undoable step and a refused one changes nothing; a reply
// is one fact per line in the key=value form the options take; errors say
// what was refused. The lines go through CommandInterpreter::run, so the
// dispatch and the tokenizer are exercised too.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

use katana_cad::plotting::generators::{
    cross_section_sheets, grid_sheets, prepare_for_append, smart_layout, strip_sheets,
};
use katana_cad::plotting::key_plan::key_plan_outlines;
use katana_cad::plotting::layout::{TILING_GUTTER_MM, tile_viewports};
use katana_cad::plotting::sheet_json::sheet_set_to_json;
use katana_cad::plotting::sheet_verbs::{
    is_sheet_verb, parse_plot_sheets, read_sheet_set_file, set_title_block_value,
    set_viewport_option, sheet_index_from, sheet_verb_help, title_block_fields, title_block_value,
};
use katana_cad::plotting::{
    CrossSectionRequest, GridRequest, LayoutRequest, Revision, Sheet, SheetSet, SheetVerbContext,
    StripRequest, TilingPreset, Viewport, ViewportKind,
};
use katana_cad::selection::drawn_extent;
use katana_cad::{CommandInterpreter, Document, NO_LAYER_OVERRIDES, PaperSize, PlotColourMode};
use katana_core::{Error, ErrorCode};
use katana_entity::Model;
use katana_geometry::alignment::solve_alignment;
use katana_geometry::{Box2, Point2};
use katana_math::DEG_TO_RAD;

use crate::{
    ScratchDirectory, Session, contains, double_eq, must, near, quoted, read_file, read_text,
    write_file,
};

// The eight-byte PNG signature and a little after it: recognised by its
// content, never decoded.
const PNG_BYTES: &[u8] = b"\x89PNG\r\n\x1a\nnot really pixels";

// The set as the project stores it.
#[track_caller]
fn json(session: &Session) -> String {
    must(sheet_set_to_json(&session.set()))
}

// The lines of `text`, split at every "\n" (an empty last one kept).
fn lines(text: &str) -> Vec<String> {
    text.split('\n').map(String::from).collect()
}

// The words of a PLOTSHEETS line after the verb.
fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(|word| String::from(*word)).collect()
}

mod sheet_verbs {
    use super::*;

    // ---- the command line --------------------------------------------------------------

    #[test]
    fn the_interpreter_hands_sheet_verbs_on_and_lists_them_in_its_help() {
        for verb in [
            "SHEETS",
            "sheet",
            "View",
            "VIEWPORT",
            "tile",
            "GENERATE",
            "titleblock",
        ] {
            assert!(is_sheet_verb(verb), "{verb}");
        }
        assert!(!is_sheet_verb("LINE"));
        assert!(!is_sheet_verb("PLOTSHEETS")); // the desktop window's own
        let help = CommandInterpreter::help_text();
        assert!(contains(&help, "Sheets    SHEETS [LIST]"));
        assert!(contains(&help, "PLOTSHEETS path.pdf"));
        // katana_cli prints this help and refuses PLOTSHEETS, so the help says
        // whose verb it is rather than advertise one the reader cannot run.
        assert!(contains(&help, "the desktop window's alone"));
        assert!(contains(&help, "katana_cli refuses it"));

        let mut session = Session::default();
        let sheet_help = session.ok("HELP SHEETS");
        assert_eq!(sheet_help, sheet_verb_help());
        assert_eq!(session.ok("help generate"), sheet_verb_help());
        for line in [
            "SHEETS [LIST]",
            "SHEETS JSON [path]",
            "SHEET NEW",
            "SHEET REMOVE n",
            "SHEET SET n",
            "SHEET FIELD n",
            "VIEW ADD n kind",
            "VIEW SET id",
            "TILE n preset",
            "GENERATE kind",
            "TITLEBLOCK [LIST]",
            "TITLEBLOCK REVISION ADD",
            "TITLEBLOCK LOGO path",
            "PLOTSHEETS",
        ] {
            assert!(contains(&sheet_help, line), "{line}");
        }
    }

    #[test]
    fn the_tokenizer_is_the_interpreters_own() {
        let words = must(CommandInterpreter::tokenize(
            r#"PLOTSHEETS "C:/my plots/set.pdf" sheets=1,3-5 """#,
        ));
        assert_eq!(
            words,
            ["PLOTSHEETS", "C:/my plots/set.pdf", "sheets=1,3-5", ""]
        );
        let open = CommandInterpreter::tokenize(r#"SHEET RENAME 1 "never closed"#)
            .expect_err("an open quote is refused");
        assert_eq!(open.code, ErrorCode::ParseFailure);
    }

    // ---- SHEET -------------------------------------------------------------------------

    #[test]
    fn a_new_sheet_is_one_step_taking_its_paper_orientation_and_place() {
        let mut s = Session::default();
        assert_eq!(
            s.ok(r#"SHEET NEW "GENERAL ARRANGEMENT" paper=A1 portrait"#),
            concat!(
                r#"added sheet 1 id=s1 name="GENERAL ARRANGEMENT" paper=A1 orientation=portrait "#,
                r#"frame=a3_landscape legendblock=on views=0"#
            )
        );
        assert_eq!(s.steps(), 1);
        assert_eq!(s.document.history().undo_name(), "ADD_SHEET");
        // Unquoted words are the name too; no name at all numbers the sheet.
        assert_eq!(
            s.ok("sheet new DRAINAGE PLAN frame=off legendblock=off"),
            concat!(
                r#"added sheet 2 id=s2 name="DRAINAGE PLAN" paper=A3 orientation=landscape "#,
                r#"frame=none legendblock=off views=0"#
            )
        );
        assert_eq!(
            s.ok("SHEET NEW at=1"),
            concat!(
                r#"added sheet 1 id=s3 name="SHEET 3" paper=A3 orientation=landscape "#,
                r#"frame=a3_landscape legendblock=on views=0"#
            )
        );
        assert_eq!(
            s.ok("SHEET NEW name=PORTRAIT orientation=landscape at=4"),
            concat!(
                r#"added sheet 4 id=s4 name="PORTRAIT" paper=A3 orientation=landscape "#,
                r#"frame=a3_landscape legendblock=on views=0"#
            )
        );
        assert_eq!(s.steps(), 4);
        let set = s.set();
        assert_eq!(set.sheets.len(), 4);
        assert_eq!(set.sheets[1].paper, PaperSize::A1);
        assert!(!set.sheets[1].landscape);
        assert!(set.sheets[2].frame.is_empty());

        // A refusal says why and records nothing.
        assert!(contains(
            &s.refused("SHEET NEW at=6").message,
            "at= is 1 to 5"
        ));
        assert!(contains(
            &s.refused("SHEET NEW paper=B2").message,
            "A0, A1, A2, A3 or A4"
        ));
        assert!(contains(
            &s.refused("SHEET NEW PLAN name=PLAN").message,
            "given twice"
        ));
        assert!(contains(
            &s.refused("SHEET NEW colour=red").message,
            "no option colour="
        ));
        assert_eq!(s.steps(), 4);
        assert!(s.document.undo().is_ok());
        assert_eq!(s.set().sheets.len(), 3);
    }

    #[test]
    fn a_sheet_is_named_by_its_number_or_its_id() {
        let mut s = Session::default();
        s.ok("SHEET NEW A");
        s.ok("SHEET NEW B");
        s.ok("SHEET NEW C");
        s.ok("SHEET MOVE 3 1"); // C A B: ids s3 s1 s2
        let set = s.set();
        assert_eq!(must(sheet_index_from(&set, "1")), 0);
        assert_eq!(must(sheet_index_from(&set, "s3")), 0);
        assert_eq!(must(sheet_index_from(&set, "S2")), 2);
        let zero = sheet_index_from(&set, "0").expect_err("there is no sheet 0");
        assert_eq!(zero.code, ErrorCode::NotFound);
        assert_eq!(zero.message, "no sheet 0: the set has 3 sheets");
        assert_eq!(
            sheet_index_from(&set, "4")
                .expect_err("there is no sheet 4")
                .message,
            "no sheet 4: the set has 3 sheets"
        );
        assert_eq!(
            sheet_index_from(&set, "s9")
                .expect_err("there is no sheet s9")
                .code,
            ErrorCode::NotFound
        );
        assert_eq!(
            sheet_index_from(&set, "plan")
                .expect_err("plan names no sheet")
                .code,
            ErrorCode::ParseFailure
        );
        assert_eq!(
            s.refused("SHEET REMOVE 7").message,
            "no sheet 7: the set has 3 sheets"
        );
    }

    #[test]
    fn remove_move_copy_and_rename_are_each_one_step() {
        let mut s = Session::default();
        s.ok("SHEET NEW A");
        s.ok("SHEET NEW B");
        s.ok("SHEET NEW C");
        let mut steps = s.steps();

        assert_eq!(s.ok("SHEET MOVE 1 3"), "moved sheet id=s1 from 1 to 3");
        steps += 1;
        assert_eq!(s.steps(), steps);
        assert_eq!(s.document.history().undo_name(), "MOVE_SHEET");

        assert_eq!(
            s.ok("SHEET COPY s2"),
            concat!(
                r#"copied sheet 1 to sheet 2 id=s4 name="B (copy)" paper=A3 orientation=landscape "#,
                r#"frame=a3_landscape legendblock=on views=0"#
            )
        );
        steps += 1;
        assert_eq!(s.steps(), steps);

        assert_eq!(
            s.ok("SHEET RENAME 2 LONG SECTION CH 0 TO 500"),
            concat!(
                r#"renamed sheet 2 id=s4 name="LONG SECTION CH 0 TO 500" paper=A3 "#,
                r#"orientation=landscape frame=a3_landscape legendblock=on views=0"#
            )
        );
        steps += 1;
        assert_eq!(s.steps(), steps);
        assert_eq!(s.document.history().undo_name(), "RENAME_SHEET");

        assert_eq!(s.ok("SHEET REMOVE s3"), r#"removed sheet 3 id=s3 name="C""#);
        steps += 1;
        assert_eq!(s.steps(), steps);

        assert_eq!(
            s.refused(r#"SHEET RENAME 1 """#).message,
            "a sheet needs a name"
        );
        assert_eq!(
            s.refused("SHEET MOVE 1 9").message,
            "no position 9: the set has 3 sheets"
        );
        assert!(contains(
            &s.refused("SHEET MOVE 1").message,
            "usage: SHEET MOVE n to"
        ));
        assert!(contains(
            &s.refused("SHEET SPIN 1").message,
            "usage: SHEET NEW"
        ));
        assert_eq!(s.steps(), steps);

        // Undo walks each back exactly.
        assert!(s.document.undo().is_ok());
        assert!(s.document.undo().is_ok());
        assert!(s.document.undo().is_ok());
        assert!(s.document.undo().is_ok());
        let set = s.set();
        assert_eq!(set.sheets.len(), 3);
        assert_eq!(set.sheets[0].name, "A");
    }

    #[test]
    fn sheet_set_changes_the_paper_and_frame_in_one_step_or_nothing() {
        let mut s = Session::default();
        s.ok("SHEET NEW PLAN");
        let before = json(&s);
        let steps = s.steps();
        assert_eq!(
            s.ok("SHEET SET 1 paper=a1 portrait frame=off legendblock=off name=SITE"),
            concat!(
                r#"set sheet 1 id=s1 name="SITE" paper=A1 orientation=portrait frame=none "#,
                r#"legendblock=off views=0"#
            )
        );
        assert_eq!(s.steps(), steps + 1);
        assert_eq!(s.document.history().undo_name(), "EDIT_SHEET");
        assert_eq!(
            s.ok("SHEET SET 1 orientation=LANDSCAPE frame=on"),
            concat!(
                r#"set sheet 1 id=s1 name="SITE" paper=A1 orientation=landscape "#,
                r#"frame=a3_landscape legendblock=off views=0"#
            )
        );

        // One bad option refuses the lot: the good one before it is not kept.
        let good = json(&s);
        assert!(contains(
            &s.refused("SHEET SET 1 paper=A2 frame=title_block_b")
                .message,
            "no plot frame of that name"
        ));
        assert!(contains(
            &s.refused("SHEET SET 1 legendblock=maybe").message,
            "on or off"
        ));
        assert!(contains(
            &s.refused("SHEET SET 1 sideways").message,
            "option=value"
        ));
        assert_eq!(json(&s), good);
        assert_eq!(s.steps(), steps + 2);

        assert!(s.document.undo().is_ok());
        assert!(s.document.undo().is_ok());
        assert_eq!(json(&s), before);
    }

    #[test]
    fn a_sheets_own_field_overrides_the_set_and_an_empty_value_clears_it() {
        let mut s = Session::default();
        s.ok("SHEET NEW PLAN");
        assert_eq!(
            s.ok("SHEET FIELD 1 sheet_number"),
            r#"sheet 1 field sheet_number="1" automatic"#
        );
        assert_eq!(
            s.ok(r#"SHEET FIELD 1 "Sheet Number" "C-101""#),
            r#"sheet 1 field sheet_number="C-101""#
        );
        assert_eq!(s.document.history().undo_name(), "SET_SHEET_FIELD");
        assert_eq!(s.set().sheets[0].fields["sheet_number"], "C-101");
        assert_eq!(
            s.ok("SHEET FIELD 1 sheet_number"),
            r#"sheet 1 field sheet_number="C-101""#
        );
        // SHEETS LIST shows the override under its sheet.
        assert!(contains(
            &s.ok("SHEETS"),
            "\n  field sheet_number=\"C-101\""
        ));
        // Unquoted words are the value.
        assert_eq!(
            s.ok("SHEET FIELD 1 scale AS SHOWN"),
            r#"sheet 1 field scale="AS SHOWN""#
        );
        assert_eq!(
            s.ok(r#"SHEET FIELD 1 sheet_number """#),
            "sheet 1 field sheet_number cleared"
        );
        assert!(!s.set().sheets[0].fields.contains_key("sheet_number"));

        let unknown = s.refused("SHEET FIELD 1 colour red");
        assert_eq!(unknown.code, ErrorCode::NotFound);
        assert!(contains(
            &unknown.message,
            "the frame prints no field of that name"
        ));
        assert!(contains(&unknown.message, "sheet_number"));
    }

    // ---- VIEW --------------------------------------------------------------------------

    #[test]
    fn a_view_is_added_as_the_editor_adds_one_then_given_its_options() {
        let mut s = Session::default();
        s.ok("SHEET NEW PLAN");
        // On an empty sheet a plan fills the tiling area, scaled and centred
        // automatically, with a north arrow and a scale bar.
        assert_eq!(
            s.ok("VIEW ADD 1 plan"),
            concat!(
                "added view vp1 to sheet 1\n",
                "view id=vp1 kind=plan rect=24,36,409,286 scale=auto centre=auto rotation=0 ",
                "north=on scalebar=on grid=none gridinterval=auto locked=off"
            )
        );
        assert_eq!(s.document.history().undo_name(), "ADD_VIEWPORT");
        // Beside it, a legend of its own size in the middle, and a key plan at
        // a place and scale given.
        assert_eq!(
            s.ok("VIEW ADD s1 legend"),
            concat!(
                "added view vp2 to sheet 1\n",
                "view id=vp2 kind=legend rect=176.5,111,256.5,211 legend=this_sheet locked=off"
            )
        );
        assert_eq!(
            s.ok(
                r#"VIEW ADD 1 keyplan rect=300,200,400,280 scale=1:5000 centre=150,50 title="KEY PLAN""#
            ),
            concat!(
                "added view vp3 to sheet 1\n",
                "view id=vp3 kind=key_plan rect=300,200,400,280 scale=5000 centre=150,50 ",
                "rotation=0 north=on scalebar=off grid=none gridinterval=auto locked=off ",
                "title=\"KEY PLAN\""
            )
        );
        assert_eq!(
            s.ok(r#"VIEW ADD 1 notes text="1. LEVELS ARE IN METRES.\n2. DO NOT SCALE.""#),
            concat!(
                "added view vp4 to sheet 1\n",
                "view id=vp4 kind=notes rect=171.5,121,261.5,201 locked=off ",
                "text=\"1. LEVELS ARE IN METRES.\\n2. DO NOT SCALE.\""
            )
        );
        assert_eq!(
            s.set().sheets[0].viewports[3].text,
            "1. LEVELS ARE IN METRES.\n2. DO NOT SCALE."
        );
        assert_eq!(s.steps(), 5);

        let steps = s.steps();
        assert!(contains(
            &s.refused("VIEW ADD 1 hologram").message,
            "a view is plan"
        ));
        assert_eq!(
            s.refused("VIEW ADD 1 legend scale=500").message,
            "scale= is for plan, key_plan, long_section and cross_sections views, not legend"
        );
        assert!(contains(
            &s.refused("VIEW ADD 1 plan rect=500,400,600,500").message,
            "would be off the paper: A3 landscape is 420 x 297 mm"
        ));
        assert_eq!(
            s.refused("VIEW ADD 1 plan rect=10,10,10,50").message,
            "rect= must have a width and a height"
        );
        assert!(contains(
            &s.refused("VIEW ADD 1 image").message,
            "an image view needs file=path"
        ));
        assert!(contains(
            &s.refused("VIEW ADD 1 plan north").message,
            "expected option=value"
        ));
        assert_eq!(s.refused("VIEW ADD 9 plan").code, ErrorCode::NotFound);
        assert_eq!(s.steps(), steps);
    }

    #[test]
    fn section_views_run_along_the_drawings_alignment() {
        let mut s = Session::default();
        s.ok("ALIGN NEW ROAD 0,0 1000,0");
        s.ok("ALIGN NEW CREEK 0,0 0,400");
        s.ok("SHEET NEW SECTIONS");
        // The first alignment by name, cut half way along.
        assert_eq!(
            s.ok("VIEW ADD 1 cross_sections"),
            concat!(
                "added view vp1 to sheet 1\n",
                "view id=vp1 kind=cross_sections rect=24,36,409,286 scale=200 centre=auto ",
                "alignment=\"CREEK\" from=0 to=0 interval=0 stations=200 halfwidth=20 ve=1 ",
                "locked=off"
            )
        );
        // Moved to another alignment it is cut half way along that one.
        assert!(contains(
            &s.ok("VIEW ADD 1 xs alignment=ROAD rect=30,40,200,150"),
            "alignment=\"ROAD\" from=0 to=0 interval=0 stations=500 halfwidth=20"
        ));
        assert!(contains(
            &s.ok("VIEW ADD 1 profile alignment=ROAD from=100 to=600 ve=10 rect=30,160,400,280"),
            "kind=long_section rect=30,160,400,280 scale=500 centre=auto alignment=\"ROAD\" \
             from=100 to=600 ve=10"
        ));
        assert_eq!(
            s.refused("VIEW ADD 1 xs alignment=RIVER").message,
            "the drawing has no alignment of that name"
        );
        assert_eq!(
            s.refused("VIEW ADD 1 xs halfwidth=0").message,
            "halfwidth= must be more than 0"
        );
        assert_eq!(
            s.refused("VIEW ADD 1 plan stations=10,20").message,
            "stations= is for cross_sections views, not plan"
        );
        assert_eq!(
            s.refused("VIEW ADD 1 xs stations=10,x").message,
            "stations= must be numbers separated by commas"
        );
    }

    #[test]
    fn view_set_changes_a_view_in_one_step_and_a_refusal_changes_nothing() {
        let mut s = Session::default();
        s.ok("SHEET NEW PLAN");
        s.ok("VIEW ADD 1 plan");
        let mut steps = s.steps();
        assert_eq!(
            s.ok("VIEW SET vp1 scale=1:250 centre=1000.5,2000.25 rotation=30 north=off"),
            concat!(
                "view id=vp1 kind=plan rect=24,36,409,286 scale=250 centre=1000.5,2000.25 ",
                "rotation=30 north=off scalebar=on grid=none gridinterval=auto locked=off"
            )
        );
        steps += 1;
        assert_eq!(s.steps(), steps);
        assert_eq!(s.document.history().undo_name(), "EDIT_VIEWPORT");
        let plan = s.set().sheets[0].viewports[0].clone();
        assert!(!plan.auto_scale);
        assert!(!plan.auto_centre);
        double_eq(plan.scale, 250.0);
        double_eq(plan.rotation, 30.0 * DEG_TO_RAD);

        assert!(contains(
            &s.ok("view set VP1 SCALE=auto Centre=AUTO locked=yes hide=TREES hide=FENCES"),
            "scale=auto centre=auto rotation=30 north=off scalebar=on grid=none \
             gridinterval=auto locked=on hidden=\"FENCES,TREES\""
        ));
        assert!(contains(
            &s.ok("VIEW SET vp1 show=TREES title=\"SITE PLAN\""),
            "hidden=\"FENCES\" title=\"SITE PLAN\""
        ));
        assert!(contains(
            &s.ok("VIEW SET vp1 hidden= rect=30,40,300,280"),
            "rect=30,40,300,280 scale=auto"
        ));
        assert!(s.set().sheets[0].viewports[0].hidden_layers.empty());
        steps += 3;
        assert_eq!(s.steps(), steps);

        // All or nothing: the good scale before the bad tilt is not kept.
        let before = json(&s);
        assert_eq!(
            s.refused("VIEW SET vp1 scale=100 tilt=45").message,
            "tilt= is for model_3d views, not plan"
        );
        assert_eq!(
            s.refused("VIEW SET vp1 kind=legend").message,
            "a view's kind is fixed: VIEW REMOVE it and VIEW ADD another"
        );
        assert!(contains(
            &s.refused("VIEW SET vp1 zoom=2").message,
            "no view option of that name"
        ));
        assert_eq!(
            s.refused("VIEW SET vp1 scale=-5").message,
            "scale= must be more than 0"
        );
        assert_eq!(s.refused("VIEW SET vp1 centre=5").message, "centre= is x,y");
        assert_eq!(
            s.refused("VIEW SET vp9 scale=100").message,
            "no view with that id; VIEW LIST shows them"
        );
        assert!(contains(
            &s.refused("VIEW SET vp1").message,
            "usage: VIEW SET id"
        ));
        assert_eq!(json(&s), before);
        assert_eq!(s.steps(), steps);

        // A 3D snapshot has a camera height and a turn, and nothing drawn to scale.
        s.ok("VIEW ADD 1 3d rect=300,40,400,120");
        assert!(contains(
            &s.ok("VIEW SET vp2 tilt=45 rotation=-90"),
            "kind=model_3d rect=300,40,400,120 rotation=-90 tilt=45 locked=off"
        ));
        assert_eq!(
            s.refused("VIEW SET vp2 tilt=90").message,
            "tilt= is the camera's height angle, 1 to 89 degrees"
        );
    }

    #[test]
    fn view_list_and_remove() {
        let mut s = Session::default();
        s.ok("SHEET NEW PLAN");
        s.ok("SHEET NEW NOTES");
        s.ok("VIEW ADD 1 plan");
        s.ok("VIEW ADD 2 notes");
        s.ok("VIEW ADD 2 legend rect=30,40,100,120");
        assert_eq!(
            s.ok("VIEW LIST 2"),
            concat!(
                "sheet 2 id=s2 views=2\n",
                "view id=vp2 kind=notes rect=24,36,409,286 locked=off ",
                "text=\"1. ALL DIMENSIONS ARE IN METRES UNLESS NOTED OTHERWISE.\"\n",
                "view id=vp3 kind=legend rect=30,40,100,120 legend=this_sheet locked=off"
            )
        );
        assert_eq!(lines(&s.ok("VIEW LIST")).len(), 5);
        let steps = s.steps();
        assert_eq!(s.ok("VIEW REMOVE vp2"), "removed view vp2 from sheet 2");
        assert_eq!(s.steps(), steps + 1);
        assert_eq!(s.document.history().undo_name(), "REMOVE_VIEWPORT");
        assert_eq!(s.set().sheets[1].viewports.len(), 1);
        assert_eq!(s.refused("VIEW REMOVE vp2").code, ErrorCode::NotFound);
        assert!(s.document.undo().is_ok());
        assert_eq!(s.set().sheets[1].viewports.len(), 2);
        // An id in another case is the same view, and the reply names it as stored.
        assert_eq!(s.ok("view remove VP3"), "removed view vp3 from sheet 2");
        assert_eq!(s.steps(), steps + 1);
        let set = s.set();
        assert_eq!(set.sheets[1].viewports.len(), 1);
        assert_eq!(set.sheets[1].viewports[0].id, "vp2");
        assert_eq!(Session::default().ok("VIEW LIST"), "0 sheets");
    }

    #[test]
    fn an_image_view_copies_its_file_into_the_project() {
        let scratch = ScratchDirectory::new("sheet-verbs-image");
        let image = scratch.path.join("site photo.png");
        write_file(&image, PNG_BYTES);
        let mut s = Session::default();
        s.ok("SHEET NEW PLAN");
        let line = format!("VIEW ADD 1 image file={}", quoted(&image));
        // Nowhere to keep it before the drawing is a project.
        assert_eq!(s.refused(&line).code, ErrorCode::InvalidState);
        assert_eq!(s.set().sheets[0].viewports.len(), 0);

        must(s.document.save_as(&scratch.path.join("project")));
        assert!(contains(
            &s.ok(&line),
            "kind=image rect=24,36,409,286 locked=off text=\"site_photo.png\""
        ));
        assert_eq!(
            read_file(
                &scratch
                    .path
                    .join("project")
                    .join("assets")
                    .join("site_photo.png")
            ),
            PNG_BYTES
        );
        // A file that is not an image is refused by its content.
        let text = scratch.path.join("notes.png");
        write_file(&text, b"plain text");
        assert_eq!(
            s.refused(&format!("VIEW SET vp1 file={}", quoted(&text)))
                .message,
            "an image must be a PNG, JPEG, GIF or BMP image"
        );
        assert_eq!(
            s.refused(&format!("VIEW ADD 1 plan file={}", quoted(&image)))
                .message,
            "file= is for image views, not plan"
        );
    }

    // ---- TILE --------------------------------------------------------------------------

    #[test]
    fn tile_lays_a_sheet_out_by_preset_id_or_menu_name() {
        let mut s = Session::default();
        s.ok("SHEET NEW PLAN");
        s.ok("VIEW ADD 1 legend");
        s.ok("VIEW ADD 1 plan");
        let mut expected: Sheet = s.set().sheets[0].clone();
        assert_eq!(
            tile_viewports(&mut expected, TilingPreset::MainRight, TILING_GUTTER_MM),
            2
        );

        let steps = s.steps();
        let reply = s.ok("TILE 1 Main and panel right");
        assert_eq!(lines(&reply)[0], "tiled 2 views on sheet 1 preset=sectionR");
        assert_eq!(s.set().sheets[0], expected);
        assert_eq!(s.steps(), steps + 1);
        assert_eq!(s.document.history().undo_name(), "TILE_VIEWPORTS");

        for preset in [
            "full",
            "COLS2",
            "rows2",
            "quad",
            "sectionb",
            "sectionBR",
            "sectionMap3d",
            "\"Two columns\"",
            "maintworight",
        ] {
            assert!(s.run(&format!("TILE s1 {preset}")).is_ok(), "{preset}");
        }
        assert!(contains(
            &s.refused("TILE 1 hexagons").message,
            "a tiling preset is full, cols2"
        ));
        assert!(contains(
            &s.refused("TILE 1").message,
            "usage: TILE n preset"
        ));
    }

    // ---- GENERATE ----------------------------------------------------------------------

    #[test]
    fn generate_fit_lays_the_drawing_out_as_the_smart_layout_does() {
        let mut s = Session::default();
        s.ok("LINE 0,0 300,100");
        let reply = s.ok("GENERATE fit");
        let request = LayoutRequest {
            plan_area: Some(drawn_extent(s.document.model(), &NO_LAYER_OVERRIDES)),
            ..LayoutRequest::default()
        };
        let expected = must(smart_layout(s.document.model(), &request, None));
        assert_eq!(s.set().sheets.len(), expected.len());
        let built = SheetSet::default();
        let mut sheets = expected;
        prepare_for_append(&built, &mut sheets);
        assert_eq!(s.set().sheets, sheets);
        assert_eq!(lines(&reply)[0], "generated 1 sheet: 1");
        assert!(contains(&reply, "\nsheet 1 id=s1 name="));
        assert!(contains(
            &reply,
            "\n  view id=vp1 kind=plan scale=1000 rect="
        ));
        assert_eq!(s.steps(), 2); // the line, and the sheets
        assert_eq!(s.document.history().undo_name(), "GENERATE_SHEETS");

        // An area given, a 3D view and legend beside it, on A1.
        assert!(contains(
            &s.ok("GENERATE fit area=0,0,300,200 model3d=on legend=on paper=A1"),
            "generated 1 sheet: 2"
        ));
        let set = s.set();
        assert_eq!(set.sheets[1].paper, PaperSize::A1);
        assert_eq!(set.sheets[1].viewports.len(), 3);
    }

    #[test]
    fn generate_uses_the_front_ends_extent_and_only_when_it_needs_one() {
        let mut document = Document::default();
        let mut interpreter = CommandInterpreter::new();
        let asked = Rc::new(Cell::new(0));
        let counter = Rc::clone(&asked);
        interpreter.set_sheet_context(Some(Box::new(
            move |_document: &Document| -> SheetVerbContext<'static> {
                counter.set(counter.get() + 1);
                SheetVerbContext {
                    drawing_extent: Some(Box2::new(
                        Point2::new(0.0, 0.0),
                        Point2::new(3000.0, 1000.0),
                    )),
                    ..SheetVerbContext::default()
                }
            },
        )));
        assert!(interpreter.run(&mut document, "SHEET NEW A").is_ok());
        assert!(interpreter.run(&mut document, "SHEETS").is_ok());
        assert_eq!(asked.get(), 0);
        // An empty drawing, but the front end draws 3 km of imagery.
        let reply = must(interpreter.run(&mut document, "GENERATE fit"));
        assert_eq!(asked.get(), 1);
        assert!(contains(&reply, "kind=plan scale=10000"));
        // Headless, an empty drawing has nothing to fit.
        let mut headless = Session::default();
        assert!(contains(
            &headless.refused("GENERATE fit").message,
            "the drawing is empty"
        ));
    }

    #[test]
    fn generate_grid_strips_profile_and_sections_match_their_generators() {
        let mut s = Session::default();
        s.ok("ALIGN NEW ROAD 0,0 1000,0");

        // Tiles: 400 x 200 m at 1:500 with 10 m of overlap is six tiles and a key
        // plan, the three the road does not reach kept too (empty=on).
        assert_eq!(
            lines(&s.ok("GENERATE grid area=0,0,400,200 scale=500 overlap=10 empty=on"))[0],
            "generated 7 sheets: 1 to 7"
        );
        let grid = GridRequest {
            area: Box2::new(Point2::new(0.0, 0.0), Point2::new(400.0, 200.0)),
            overlap_m: 10.0,
            ..GridRequest::default()
        };
        let mut tiles = must(grid_sheets(&grid, None));
        prepare_for_append(&SheetSet::default(), &mut tiles);
        assert_eq!(s.set().sheets, tiles);

        // Strips along the only alignment, replacing the tiles.
        let strips = s.ok("GENERATE strips scale=500 overlap=20 replace=on");
        assert_eq!(
            lines(&strips)[0],
            "generated 6 sheets: 1 to 6 (the set was replaced)"
        );
        let strip = StripRequest {
            overlap_m: 20.0,
            ..StripRequest::default()
        };
        let road = must(solve_alignment(
            &s.document
                .model()
                .alignments
                .find("ROAD")
                .expect("ROAD")
                .horizontal,
        ));
        let mut expected_strips = must(strip_sheets(&road, "ROAD", &strip));
        prepare_for_append(&SheetSet::default(), &mut expected_strips);
        assert_eq!(s.set().sheets, expected_strips);

        // Plan and profile on auto: the whole road on one sheet.
        assert_eq!(
            lines(&s.ok("GENERATE profile alignment=ROAD"))[0],
            "generated 1 sheet: 7"
        );
        assert_eq!(s.set().sheets[6].viewports.len(), 2);
        assert_eq!(
            s.set().sheets[6].viewports[1].kind,
            ViewportKind::LongSection
        );

        // Cross sections every 100 m, 2 x 2 to a sheet: eleven sections, three sheets.
        assert_eq!(
            lines(&s.ok("GENERATE sections interval=100 rows=2 columns=2 halfwidth=15"))[0],
            "generated 3 sheets: 8 to 10"
        );
        let sections = CrossSectionRequest {
            interval: 100.0,
            rows: 2,
            columns: 2,
            half_width: 15.0,
            ..CrossSectionRequest::default()
        };
        let expected_sections = must(cross_section_sheets(&road, "ROAD", &sections));
        assert_eq!(expected_sections.len(), 3);
        let set = s.set();
        for (i, expected) in expected_sections.iter().enumerate() {
            let sheet = &set.sheets[7 + i];
            assert_eq!(sheet.viewports.len(), expected.viewports.len());
            for (made, wanted) in sheet.viewports.iter().zip(&expected.viewports) {
                assert_eq!(made.source, wanted.source);
                assert_eq!(made.rect, wanted.rect);
            }
        }
        // Stations given instead.
        assert_eq!(
            lines(&s.ok("GENERATE sections stations=10,500,990 rows=3 columns=1"))[0],
            "generated 1 sheet: 11"
        );
        assert_eq!(s.set().sheets[10].viewports.len(), 3);
    }

    #[test]
    fn generate_grid_leaves_out_tiles_with_nothing_drawn_in_them_and_says_how_many() {
        // The six tiles of 400 x 200 m at 1:500 with 10 m of overlap: columns
        // x -78.75..113.75, 103.75..296.25 and 286.25..478.75; rows y 95..220
        // and -20..105. A short line in the top-left, top-middle, bottom-middle
        // and bottom-right tiles, each clear of every overlap; the top-right and
        // bottom-left tiles have nothing drawn in them.
        let mut s = Session::default();
        for line in [
            "LINE -1,200 1,200",
            "LINE 199,200 201,200",
            "LINE 199,0 201,0",
            "LINE 399,0 401,0",
        ] {
            s.ok(line);
        }
        let steps = s.steps();
        let reply = lines(&s.ok("GENERATE grid area=0,0,400,200 scale=500 overlap=10"));
        assert_eq!(reply[0], "generated 5 sheets: 1 to 5");
        assert_eq!(
            reply[reply.len() - 1],
            "skipped 2 of 6 tiles, with nothing drawn in them"
        );
        assert_eq!(s.steps(), steps + 1);
        let set = s.set();
        assert_eq!(set.sheets.len(), 5);
        assert_eq!(set.sheets[0].name, "KEY PLAN");
        // The tiles kept, in reading order, where they were on the grid: centres
        // (17.5, 157.5), (200, 157.5), (200, 42.5) and (382.5, 42.5).
        let centres = [
            Point2::new(17.5, 157.5),
            Point2::new(200.0, 157.5),
            Point2::new(200.0, 42.5),
            Point2::new(382.5, 42.5),
        ];
        for (i, centre) in centres.iter().enumerate() {
            let tile = &set.sheets[i + 1];
            assert_eq!(tile.name, format!("PLAN TILE {}", i + 1));
            near(tile.viewports[0].centre.x, centre.x, 1e-9);
            near(tile.viewports[0].centre.y, centre.y, 1e-9);
        }
        // The live key plan outlines the four sheets there are.
        assert_eq!(key_plan_outlines(&set, 0, None).len(), 4);

        // empty=on keeps every tile.
        assert_eq!(
            lines(&s.ok("GENERATE grid area=0,0,400,200 scale=500 overlap=10 empty=on replace=on"))
                [0],
            "generated 7 sheets: 1 to 7 (the set was replaced)"
        );

        // A fit at a fixed scale too large for one sheet tiles the same way:
        // without overlap, columns from x -88.75 and rows down from y 225, the
        // lines again in four of the six.
        let fitted = lines(&s.ok("GENERATE fit area=0,0,400,200 scale=500 replace=on"));
        assert_eq!(
            fitted[0],
            "generated 5 sheets: 1 to 5 (the set was replaced)"
        );
        assert_eq!(
            fitted[fitted.len() - 1],
            "skipped 2 of 6 tiles, with nothing drawn in them"
        );
    }

    #[test]
    fn generate_refuses_tiles_of_which_none_has_anything_drawn_in_it() {
        let mut s = Session::default();
        s.ok("LINE 0,0 10,10");
        let steps = s.steps();
        let error = s.refused("GENERATE grid area=1000,1000,1400,1200 scale=500");
        assert_eq!(error.code, ErrorCode::NotFound);
        assert_eq!(
            error.message,
            "nothing is drawn in any tile over the area; empty=on keeps every tile"
        );
        assert_eq!(
            s.refused("GENERATE fit area=1000,1000,1400,1200 scale=500")
                .code,
            ErrorCode::NotFound
        );
        assert!(s.set().sheets.is_empty());
        assert_eq!(s.steps(), steps);
        // Kept on request: 400 x 200 m at 1:500 is six tiles and a key plan.
        assert_eq!(
            lines(&s.ok("GENERATE grid area=1000,1000,1400,1200 scale=500 empty=on"))[0],
            "generated 7 sheets: 1 to 7"
        );
        // A plan along an alignment is never tiled, so it takes no empty=.
        s.ok("ALIGN NEW ROAD 0,0 1000,0");
        assert_eq!(
            s.refused("GENERATE fit alignment=ROAD empty=on").message,
            "empty= is for the tiles of an area; a plan along alignment= is never tiled"
        );
    }

    #[test]
    fn generate_says_what_it_cannot_do() {
        let mut s = Session::default();
        assert_eq!(
            s.refused("GENERATE strips").message,
            "the drawing has no alignment: define one with ALIGN NEW"
        );
        s.ok("ALIGN NEW ROAD 0,0 1000,0");
        s.ok("ALIGN NEW CREEK 0,0 0,400");
        assert_eq!(
            s.refused("GENERATE sections").message,
            "name the alignment with alignment=: the drawing has CREEK, ROAD"
        );
        assert_eq!(
            s.refused("GENERATE strips alignment=RIVER").message,
            "the drawing has no alignment of that name"
        );
        assert_eq!(
            s.refused("GENERATE mosaic").message,
            "GENERATE makes fit, grid, strips, profile, sections, frames or register"
        );
        assert_eq!(
            s.refused("GENERATE grid interval=20").message,
            "GENERATE GRID takes no option interval=; it takes paper orientation frame area \
             scale overlap keyplan empty replace"
        );
        assert!(contains(
            &s.refused("GENERATE grid area=0,0,10,10 scale=auto").message,
            "GENERATE GRID needs a scale"
        ));
        assert!(contains(
            &s.refused("GENERATE strips alignment=ROAD overlap=10")
                .message,
            "need a fixed scale"
        ));
        assert_eq!(
            s.refused("GENERATE sections alignment=ROAD interval=20 stations=5")
                .message,
            "GENERATE SECTIONS cuts every interval= or at stations=, not both"
        );
        assert_eq!(
            s.refused("GENERATE fit area=0,0,10,10 alignment=ROAD")
                .message,
            "GENERATE FIT lays out area= or alignment=, not both"
        );
        assert_eq!(s.refused("GENERATE frames").code, ErrorCode::NotFound);
        assert_eq!(
            s.refused("GENERATE grid area=0,0,400,200 overlap=-1")
                .message,
            "overlap= cannot be negative"
        );
        assert_eq!(
            s.refused("GENERATE frames portrait").code,
            ErrorCode::InvalidArgument
        );
        assert!(s.set().sheets.is_empty());
        assert_eq!(s.steps(), 2); // the two alignments, and nothing else
    }

    // ---- TITLEBLOCK --------------------------------------------------------------------

    #[test]
    fn the_title_block_is_listed_and_set_field_by_field() {
        let mut s = Session::default();
        let blank = lines(&s.ok("TITLEBLOCK"));
        assert_eq!(blank.len(), title_block_fields().len() + 2);
        assert_eq!(blank[0], "organisation=\"\"");
        assert_eq!(blank[7], "numbering=\"{n}\"");
        assert_eq!(blank[blank.len() - 2], "logo=\"\"");
        assert_eq!(blank[blank.len() - 1], "0 revisions");

        assert_eq!(
            s.ok("TITLEBLOCK organisation Example Surveys"),
            "organisation=\"Example Surveys\""
        );
        assert_eq!(s.steps(), 1);
        assert_eq!(s.document.history().undo_name(), "EDIT_TITLE_BLOCK");
        assert_eq!(
            s.ok("TITLEBLOCK organization"),
            "organisation=\"Example Surveys\""
        );
        assert_eq!(
            s.ok("TITLEBLOCK project_line_2 \"Stage 2\""),
            "project2=\"Stage 2\""
        );
        assert_eq!(
            s.ok("TITLEBLOCK SurveyorName J. CITIZEN"),
            "surveyorname=\"J. CITIZEN\""
        );
        assert_eq!(
            s.ok("TITLEBLOCK surveyor_date 02/01/26"),
            "surveyordate=\"02/01/26\""
        );
        assert_eq!(
            s.ok("TITLEBLOCK coordinate_system GRID 56"),
            "coordsys=\"GRID 56\""
        );
        assert_eq!(s.ok("TITLEBLOCK datum LOCAL"), "datum=\"LOCAL\"");
        assert_eq!(
            s.ok("TITLEBLOCK notes \"Services are approximate.\\nConfirm before digging.\""),
            "notes=\"Services are approximate.\\nConfirm before digging.\""
        );
        assert_eq!(
            s.ok("TITLEBLOCK numbering {set}-{n:02}"),
            "numbering=\"{set}-{n:02}\""
        );
        assert_eq!(s.ok("TITLEBLOCK set DS-0001"), "setnumber=\"DS-0001\"");

        {
            let set = s.set();
            let d = &set.defaults;
            assert_eq!(d.organisation, "Example Surveys");
            assert_eq!(d.project_lines, ["", "Stage 2"]);
            assert_eq!(d.surveyor.name, "J. CITIZEN");
            assert_eq!(d.surveyor.date, "02/01/26");
            assert_eq!(d.coordinate_system, "GRID 56");
            assert_eq!(d.height_datum, "LOCAL");
            assert_eq!(
                d.notes,
                "Services are approximate.\nConfirm before digging."
            );
            assert_eq!(set.numbering, "{set}-{n:02}");
            assert_eq!(d.set_number, "DS-0001");
        }
        assert_eq!(s.steps(), 9); // nine values set; the one read recorded nothing

        // A project line cleared leaves no empty lines trailing, so the set can
        // go back to its default.
        s.ok("TITLEBLOCK project2 \"\"");
        assert!(s.set().defaults.project_lines.is_empty());
        // An empty numbering is the default; one without {n} numbers nothing.
        assert_eq!(s.ok("TITLEBLOCK numbering \"\""), "numbering=\"{n}\"");
        assert!(contains(
            &s.refused("TITLEBLOCK numbering DRAWING").message,
            "needs {n}"
        ));
        assert_eq!(
            s.refused("TITLEBLOCK colour red").message,
            "no title-block field of that name; TITLEBLOCK LIST shows them"
        );

        let listed = lines(&s.ok("TITLEBLOCK LIST"));
        assert_eq!(listed[0], "organisation=\"Example Surveys\"");
        assert_eq!(listed[14], "surveyorname=\"J. CITIZEN\"");
    }

    #[test]
    fn revisions_are_added_and_removed_by_code() {
        let mut s = Session::default();
        assert_eq!(
            s.ok("TITLEBLOCK REVISION ADD A 01/02/26 \"First issue\" JC"),
            "added revision code=\"A\" date=\"01/02/26\" description=\"First issue\" by=\"JC\""
        );
        assert_eq!(s.document.history().undo_name(), "ADD_REVISION");
        s.ok("TITLEBLOCK REVISION ADD B 03/02/26 Kerbs");
        {
            let set = s.set();
            assert_eq!(set.revisions.len(), 2);
            assert_eq!(
                set.revisions[1],
                Revision {
                    code: String::from("B"),
                    date: String::from("03/02/26"),
                    description: String::from("Kerbs"),
                    by: String::new(),
                }
            );
        }
        assert!(contains(
            &s.ok("TITLEBLOCK"),
            "\n2 revisions\nrevision code=\"A\" date=\"01/02/26\" description=\"First issue\" \
             by=\"JC\"\nrevision code=\"B\""
        ));
        // The revisions alone.
        let revisions = concat!(
            "2 revisions\n",
            "revision code=\"A\" date=\"01/02/26\" description=\"First issue\" by=\"JC\"\n",
            "revision code=\"B\" date=\"03/02/26\" description=\"Kerbs\" by=\"\""
        );
        assert_eq!(s.ok("TITLEBLOCK REVISION"), revisions);
        assert_eq!(s.ok("titleblock revisions list"), revisions);
        assert!(contains(
            &s.refused("TITLEBLOCK REVISION LIST A").message,
            "usage"
        ));
        assert!(contains(
            &s.refused("TITLEBLOCK REVISION RENAME A B").message,
            "usage"
        ));
        let twice = s.refused("TITLEBLOCK REVISION ADD A 04/02/26 Again");
        assert_eq!(twice.code, ErrorCode::AlreadyExists);
        assert!(contains(
            &s.refused("TITLEBLOCK REVISION ADD C 04/02/26").message,
            "usage"
        ));

        assert_eq!(
            s.ok("TITLEBLOCK REVISION REMOVE A"),
            "removed revision code=\"A\""
        );
        assert_eq!(s.document.history().undo_name(), "REMOVE_REVISION");
        assert_eq!(
            s.refused("TITLEBLOCK REVISION REMOVE A").code,
            ErrorCode::NotFound
        );
        assert_eq!(s.set().revisions.len(), 1);
        assert!(s.document.undo().is_ok());
        assert_eq!(s.set().revisions.len(), 2);
    }

    #[test]
    fn the_logo_is_imported_into_the_project_and_can_be_removed() {
        let scratch = ScratchDirectory::new("sheet-verbs-logo");
        let logo = scratch.path.join("our logo.png");
        write_file(&logo, PNG_BYTES);
        let mut s = Session::default();
        let line = format!("TITLEBLOCK LOGO {}", quoted(&logo));
        assert_eq!(s.refused(&line).code, ErrorCode::InvalidState);
        must(s.document.save_as(&scratch.path.join("project")));
        assert_eq!(s.ok(&line), "logo=\"our_logo.png\"");
        assert_eq!(s.document.history().undo_name(), "SET_LOGO");
        assert_eq!(s.set().defaults.logo_asset, "our_logo.png");
        assert_eq!(
            read_file(
                &scratch
                    .path
                    .join("project")
                    .join("assets")
                    .join("our_logo.png")
            ),
            PNG_BYTES
        );
        assert_eq!(s.ok("TITLEBLOCK LOGO \"\""), "logo=\"\"");
        assert!(s.set().defaults.logo_asset.is_empty());
        assert_eq!(
            s.refused(&format!(
                "TITLEBLOCK LOGO {}",
                quoted(&scratch.path.join("missing.png"))
            ))
            .code,
            ErrorCode::NotFound
        );
    }

    // ---- SHEETS ------------------------------------------------------------------------

    #[test]
    fn sheets_lists_every_sheet_and_its_views() {
        let mut s = Session::default();
        assert_eq!(s.ok("SHEETS"), "0 sheets");
        s.ok("SHEET NEW PLAN");
        s.ok("SHEET NEW \"KEY PLAN\" paper=A4 portrait");
        s.ok("VIEW ADD 1 plan scale=500 centre=0,0");
        s.ok("VIEW ADD 1 legend");
        s.ok("VIEW ADD 2 key_plan");
        assert_eq!(
            s.ok("SHEETS LIST"),
            concat!(
                "2 sheets\n",
                "sheet 1 id=s1 name=\"PLAN\" paper=A3 orientation=landscape frame=a3_landscape ",
                "legendblock=on views=2\n",
                "  view id=vp1 kind=plan scale=500 rect=24,36,409,286\n",
                "  view id=vp2 kind=legend rect=176.5,111,256.5,211\n",
                "sheet 2 id=s2 name=\"KEY PLAN\" paper=A4 orientation=portrait ",
                "frame=a3_landscape legendblock=on views=1\n",
                "  view id=vp3 kind=key_plan scale=auto rect=11,11,199,286"
            )
        );
        // The key plan stores no outlines: it draws the other sheets' plans
        // where they are when it is drawn (key_plan.rs), so none goes stale.
        let set = s.set();
        let key = &set.sheets[1].viewports[0];
        assert!(key.marks.is_empty());
        assert!(key.auto_scale && key.auto_centre);
    }

    #[test]
    fn sheets_json_save_and_load_round_trip_the_whole_set() {
        let scratch = ScratchDirectory::new("sheet-verbs-json");
        let mut s = Session::default();
        s.ok("ALIGN NEW ROAD 0,0 1000,0");
        s.ok("GENERATE strips scale=500 overlap=20 keyplan=on");
        s.ok("TITLEBLOCK organisation \"Example Surveys\"");
        s.ok("TITLEBLOCK REVISION ADD A 01/02/26 \"First issue\" JC");
        s.ok("SHEET FIELD 2 sheet_number C-101");
        s.ok("VIEW SET vp2 title=\"STRIP ONE\" hide=FRAMES");

        // Printed, it is exactly what the project stores.
        assert_eq!(s.ok("SHEETS JSON"), json(&s));

        let file = scratch.path.join("set one.json");
        let path = quoted(&file);
        assert_eq!(
            s.ok(&format!("SHEETS SAVE {path}")),
            format!("wrote 7 sheets to {path}")
        );
        assert_eq!(read_text(&file), format!("{}\n", json(&s)));
        assert_eq!(
            s.ok(&format!("SHEETS JSON {path}")),
            format!("wrote 7 sheets to {path}")
        );
        let read = must(read_sheet_set_file(&file));
        assert_eq!(read, *s.set());

        // Loaded into another drawing it is the same set, as one step.
        let mut other = Session::default();
        other.ok("SHEET NEW SCRAP");
        assert_eq!(
            other.ok(&format!("SHEETS LOAD {path}")),
            format!("loaded 7 sheets from {path}")
        );
        assert_eq!(other.steps(), 2);
        assert_eq!(other.document.history().undo_name(), "LOAD_SHEETS");
        assert_eq!(*other.set(), *s.set());
        assert_eq!(json(&other), json(&s));
        assert!(other.document.undo().is_ok());
        let set = other.set();
        assert_eq!(set.sheets.len(), 1);
        assert_eq!(set.sheets[0].name, "SCRAP");

        // What cannot be loaded changes nothing and says why.
        let before = json(&other);
        write_file(
            &scratch.path.join("newer.json"),
            br#"{"format": "katana-sheets", "version": 99}"#,
        );
        assert_eq!(
            other
                .refused(&format!(
                    "SHEETS LOAD {}",
                    quoted(&scratch.path.join("newer.json"))
                ))
                .code,
            ErrorCode::Unsupported
        );
        write_file(&scratch.path.join("junk.json"), b"not json");
        assert_eq!(
            other
                .refused(&format!(
                    "SHEETS LOAD {}",
                    quoted(&scratch.path.join("junk.json"))
                ))
                .code,
            ErrorCode::ParseFailure
        );
        assert_eq!(
            other
                .refused(&format!(
                    "SHEETS LOAD {}",
                    quoted(&scratch.path.join("none.json"))
                ))
                .code,
            ErrorCode::NotFound
        );
        assert_eq!(
            other
                .refused(&format!(
                    "SHEETS SAVE {}",
                    quoted(&scratch.path.join("no").join("such").join("dir.json"))
                ))
                .code,
            ErrorCode::FileExportFailure
        );
        assert!(contains(
            &other.refused("SHEETS SAVE").message,
            "usage: SHEETS"
        ));
        assert_eq!(json(&other), before);
    }

    #[test]
    fn sheets_that_cannot_be_read_are_reported_not_listed_as_none() {
        let mut s = Session::default();
        let mut metadata = s.document.metadata().clone();
        metadata.unknown_keys.insert(
            String::from("sheets"),
            String::from(r#"{"format": "katana-sheets", "version": 99}"#),
        );
        s.document.set_metadata(metadata);
        let error = s.refused("SHEETS");
        assert_eq!(error.code, ErrorCode::Unsupported);
        assert!(contains(
            &error.message,
            "the project's sheets cannot be read"
        ));
        assert_eq!(s.refused("SHEETS JSON").code, ErrorCode::Unsupported);
        assert_eq!(s.refused("TITLEBLOCK").code, ErrorCode::Unsupported);
        // And an edit is refused rather than overwriting them.
        assert_eq!(s.refused("SHEET NEW A").code, ErrorCode::Unsupported);
        assert_eq!(
            s.document.metadata().unknown_keys["sheets"],
            r#"{"format": "katana-sheets", "version": 99}"#
        );
    }

    #[test]
    fn sheets_that_are_not_valid_json_are_neither_listed_nor_overwritten() {
        // Broken text is no set at all, not an empty one: every verb that reads
        // or edits the sheets is refused and the text is kept as it was. SHEETS
        // LOAD replaces them on purpose, as one step that UNDO takes back.
        let mut s = Session::default();
        let broken = r#"{"format": "katana-sheets", "version": 1, "sheets": ["#;
        let mut metadata = s.document.metadata().clone();
        metadata
            .unknown_keys
            .insert(String::from("sheets"), String::from(broken));
        s.document.set_metadata(metadata);
        let steps = s.steps();
        for line in [
            "SHEETS",
            "SHEET NEW A",
            "GENERATE grid area=0,0,100,100",
            "TITLEBLOCK organisation X",
            "VIEW LIST",
            "TILE 1 full",
            "TITLEBLOCK REVISION ADD A 01/01/26 FIRST",
        ] {
            let error = s.refused(line);
            assert_eq!(error.code, ErrorCode::ParseFailure, "{line}");
            assert!(
                contains(&error.message, "the project's sheets cannot be read"),
                "{line}"
            );
        }
        assert_eq!(s.document.metadata().unknown_keys["sheets"], broken);
        assert_eq!(s.steps(), steps);
    }

    #[test]
    fn a_text_typed_back_is_the_text_stored() {
        let mut s = Session::default();
        // "\n" is a line break and "\\" a backslash; the reply writes them so.
        assert_eq!(
            s.ok(r#"TITLEBLOCK notes "SEE C:\\NEW\nLEVELS IN METRES""#),
            r#"notes="SEE C:\\NEW\nLEVELS IN METRES""#
        );
        assert_eq!(s.set().defaults.notes, "SEE C:\\NEW\nLEVELS IN METRES");
        // What the reply says, typed back, stores the same text.
        let reply = s.ok("TITLEBLOCK notes");
        let equals = reply.find('=').expect("a field=value reply");
        s.ok(&format!(
            "TITLEBLOCK {} {}",
            &reply[..equals],
            &reply[equals + 1..]
        ));
        assert_eq!(s.set().defaults.notes, "SEE C:\\NEW\nLEVELS IN METRES");
        // A backslash before anything else is itself.
        s.ok(r#"SHEET NEW "A\B""#);
        assert_eq!(
            s.set().sheets.last().map(|sheet| sheet.name.clone()),
            Some(String::from("A\\B"))
        );
        assert!(contains(&s.ok("SHEETS"), r#"name="A\\B""#));
    }

    // ---- every edit is one step, and the same lines make the same set --------------------

    #[test]
    fn every_edit_is_one_undo_step_that_undoes_exactly() {
        let scratch = ScratchDirectory::new("sheet-verbs-steps");
        let image = scratch.path.join("photo.png");
        write_file(&image, PNG_BYTES);
        let mut s = Session::default();
        s.ok("ALIGN NEW ROAD 0,0 1000,0");
        must(s.document.save_as(&scratch.path.join("project")));
        let edits = [
            String::from("SHEET NEW PLAN"),
            String::from("SHEET NEW \"LONG SECTION\" paper=A1"),
            String::from("SHEET MOVE 2 1"),
            String::from("SHEET COPY 1"),
            String::from("SHEET RENAME 3 SITE"),
            String::from("SHEET SET 3 paper=A2 legendblock=off"),
            String::from("SHEET FIELD 3 scale \"AS SHOWN\""),
            String::from("SHEET REMOVE 2"),
            String::from("VIEW ADD 1 profile"),
            String::from("VIEW ADD 2 plan"),
            format!("VIEW ADD 2 image file={}", quoted(&image)),
            String::from("VIEW SET vp2 scale=1000 centre=500,0 rotation=15"),
            String::from("VIEW REMOVE vp1"),
            String::from("TILE 2 sectionR"),
            String::from("GENERATE sections interval=250"),
            String::from("GENERATE strips scale=1000 replace=on"),
            String::from("TITLEBLOCK client \"Client Pty Ltd\""),
            String::from("TITLEBLOCK REVISION ADD A 01/02/26 \"First issue\""),
            String::from("TITLEBLOCK REVISION REMOVE A"),
            format!("TITLEBLOCK LOGO {}", quoted(&image)),
        ];
        let mut states = vec![json(&s)];
        for line in &edits {
            let before = s.steps();
            s.ok(line);
            assert_eq!(s.steps(), before + 1, "{line}");
            states.push(json(&s));
        }
        // Back through every one, each undo landing exactly on the state before.
        for (edit, state) in edits.iter().zip(&states).rev() {
            assert!(s.document.undo().is_ok(), "{edit}");
            assert_eq!(json(&s), *state, "undoing {edit}");
        }
        // And forward again.
        for (edit, state) in edits.iter().zip(&states[1..]) {
            assert!(s.document.redo().is_ok());
            assert_eq!(json(&s), *state, "redoing {edit}");
        }
    }

    #[test]
    fn the_same_lines_make_the_same_set() {
        fn build() -> String {
            let mut s = Session::default();
            for line in [
                "ALIGN NEW ROAD 0,0 400,0 400,300",
                "LINE -50,-50 450,350",
                "GENERATE fit model3d=on legend=on",
                "GENERATE profile scale=500 interval=50",
                "GENERATE grid scale=1000 overlap=5",
                "SHEET MOVE 3 1",
                "TILE 2 quad",
                "VIEW ADD 1 notes text=\"GENERAL NOTES\"",
            ] {
                s.ok(line);
            }
            format!("{}\n{}", json(&s), s.ok("SHEETS LIST"))
        }
        assert_eq!(build(), build());
    }

    // ---- the pieces --------------------------------------------------------------------

    // PLOTSHEETS reads its words as the Plot dialog and --plot-sheets read
    // theirs: the sheets as every plot's selection (page_setup.rs), and what is
    // not given left to the page setup.
    #[test]
    fn plot_sheets_reads_the_words_every_plot_takes() {
        let set = SheetSet {
            sheets: ["s1", "s2", "s3", "s4", "s5", "s6"]
                .iter()
                .map(|id| Sheet {
                    id: String::from(*id),
                    ..Sheet::default()
                })
                .collect(),
            ..SheetSet::default()
        };
        let request = must(parse_plot_sheets(
            &set,
            &words(&["C:/plots/set.pdf", "sheets=2,4-6", "dpi=150"]),
        ));
        assert_eq!(request.path, PathBuf::from("C:/plots/set.pdf"));
        assert_eq!(request.sheets, "2,4-6");
        assert_eq!(request.dpi, Some(150.0));
        assert!(request.format.is_none());
        let every = must(parse_plot_sheets(&set, &words(&["out.pdf"])));
        assert!(every.sheets.is_empty());
        assert!(
            every.dpi.is_none()
                && every.colour_mode.is_none()
                && every.line_weight_scale.is_none()
                && every.file_name_pattern.is_none(),
            "the page setup's"
        );

        let pngs = must(parse_plot_sheets(
            &set,
            &words(&[
                "format=png",
                "style=mono",
                "folder=out",
                "pattern={n:02}",
                "lineweight=1.4",
                "sheets=s2",
            ]),
        ));
        assert_eq!(pngs.path, PathBuf::from("out"));
        assert_eq!(pngs.format.as_deref(), Some("png"));
        assert_eq!(pngs.colour_mode, Some(PlotColourMode::Monochrome));
        assert_eq!(pngs.file_name_pattern.as_deref(), Some("{n:02}"));
        assert_eq!(pngs.line_weight_scale, Some(1.4));
        assert_eq!(pngs.sheets, "s2");
        // folder= alone is a PDF a sheet.
        assert_eq!(
            must(parse_plot_sheets(&set, &words(&["folder=out"])))
                .format
                .as_deref(),
            Some("pdfs")
        );

        let refusal = |args: &[&str]| -> Error {
            parse_plot_sheets(&set, &words(args)).expect_err("the words are refused")
        };
        assert!(contains(&refusal(&[]).message, "usage: PLOTSHEETS"));
        assert_eq!(
            refusal(&["out.pdf", "dpi=20"]).message,
            "dpi= is 50 to 1200"
        );
        assert!(contains(&refusal(&["out.pdf", "sheets=7"]).message, "7"));
        assert!(contains(
            &refusal(&["out.pdf", "format=bmp"]).message,
            "pdf, pdfs, png or tiff"
        ));
        assert!(contains(
            &refusal(&["out.pdf", "folder=x"]).message,
            "not both"
        ));
        assert!(parse_plot_sheets(&set, &words(&["out.pdf", "pattern={nope}"])).is_err());
        assert!(contains(
            &refusal(&["out.pdf", "paper=A1"]).message,
            "takes no option paper="
        ));
    }

    #[test]
    fn the_title_block_functions_read_and_write_every_field() {
        let mut set = SheetSet::default();
        for field in title_block_fields() {
            let value = if field == "numbering" {
                String::from("{n} of {N}")
            } else {
                format!("value of {field}")
            };
            must(set_title_block_value(&mut set, field, value.clone()));
            assert_eq!(must(title_block_value(&set, field)), value, "{field}");
        }
        assert_eq!(set.defaults.project_lines.len(), 4);
        assert_eq!(set.defaults.approver.date, "value of approverdate");
        assert_eq!(set.defaults.locator.name, "value of locatorname");
        assert_eq!(set.defaults.model_name, "value of model");
        // Every field reached a member of its own: the JSON holds all 22.
        let json = must(sheet_set_to_json(&set));
        for field in title_block_fields() {
            if field != "numbering" {
                assert!(contains(&json, &format!("value of {field}")), "{field}");
            }
        }
        assert_eq!(
            must(title_block_value(&set, "height_datum")),
            "value of datum"
        );
        assert_eq!(
            title_block_value(&set, "nothing")
                .expect_err("there is no such field")
                .code,
            ErrorCode::NotFound
        );
    }

    #[test]
    fn a_view_option_is_checked_before_it_is_applied() {
        let model = Model::default();
        let mut plan = Viewport {
            kind: ViewportKind::Plan,
            ..Viewport::default()
        };
        must(set_viewport_option(&mut plan, "Scale", "1:2500", &model));
        double_eq(plan.scale, 2500.0);
        must(set_viewport_option(
            &mut plan,
            "rect",
            "100,80,20,10",
            &model,
        ));
        assert_eq!(
            plan.rect,
            Box2::new(Point2::new(20.0, 10.0), Point2::new(100.0, 80.0))
        ); // either corner first
        let before = plan.clone();
        assert!(set_viewport_option(&mut plan, "rotation", "north", &model).is_err());
        assert!(set_viewport_option(&mut plan, "alignment", "ROAD", &model).is_err());
        assert!(set_viewport_option(&mut plan, "scale", "1:0", &model).is_err());
        assert!(set_viewport_option(&mut plan, "centre", "nan,1", &model).is_err());
        assert_eq!(plan, before);
    }
}
