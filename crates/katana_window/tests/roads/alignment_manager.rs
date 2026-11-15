//! The twins of tests/qt_widgets/test_alignment_manager.cpp: Terrain > Alignment Manager, driven
//! by its object names as a person drives it by clicking and typing.
//!
//! The dialog changes nothing itself: it writes the ALIGN (or LABEL ALIGN) line a person would
//! type and hands it to the window's executor. So what is tested here is each line - exactly the
//! one a person would type - that the drawing then holds what the line says, that each edit is one
//! undo step, and that what the dialog shows is what the model and the setting-out function hold.
//! The executor here runs the line through the real CommandInterpreter on the same drawing, which
//! is where the window's command line sends an ALIGN line.
//!
//! Only the chosen tab's page is drawn (alignment_manager's module comment), so a twin chooses the
//! tab, as a person does, before it acts on the Vertical or Setting Out page; the C++ tests clicked
//! the hidden page's buttons directly.

use katana_cad::Document;
use katana_cad::alignment_report::{setting_out_csv, setting_out_stations};
use katana_commands::entity_commands::create_polyline;
use katana_geometry::alignment::solve_alignment;
use katana_geometry::{Point2, Polyline2};
use katana_window::alignment_manager::AlignmentManagerDialog;
use katana_window::host::{Answer, Clipboard, RecordingHost};

use crate::driver::{Driven, ok};
use crate::window_support::{Scratch, interpreter_host};

fn manager(host: RecordingHost) -> Driven<AlignmentManagerDialog> {
    let dialog = AlignmentManagerDialog::new(host.document());
    Driven::new(host, dialog, "alignmentManagerDialog")
}

/// A drawing with the corner (0,0) (100,0) (100,100) defined as "road".
fn fixture() -> Driven<AlignmentManagerDialog> {
    let mut host = interpreter_host(Document::new(None));
    ok(&mut host, "ALIGN NEW road 0,0 100,0 100,100");
    manager(host)
}

fn road(d: &Driven<AlignmentManagerDialog>) -> Option<katana_entity::Alignment> {
    d.document().model().alignments.find("road").cloned()
}

#[track_caller]
fn cell_text(d: &Driven<AlignmentManagerDialog>, table: &str, row: usize, column: usize) -> String {
    let grid = d
        .dialog
        .table(table)
        .unwrap_or_else(|| panic!("no {table}"));
    assert!(
        grid.cell(row, column).is_some(),
        "no cell {row},{column} in {table}"
    );
    grid.text(row, column)
}

fn row_count(d: &Driven<AlignmentManagerDialog>, table: &str) -> usize {
    d.dialog
        .table(table)
        .unwrap_or_else(|| panic!("no {table}"))
        .row_count()
}

/// Types into a grid cell as a person commits an edit, then lets the window run what the dialog
/// queued for it.
#[track_caller]
fn edit(
    d: &mut Driven<AlignmentManagerDialog>,
    table: &str,
    row: usize,
    column: usize,
    text: &str,
) {
    assert!(
        d.dialog.edit_cell(table, row, column, text),
        "no cell {row},{column} in {table}"
    );
    d.settle();
}

fn status(d: &Driven<AlignmentManagerDialog>) -> String {
    d.text("alignmentStatus")
}

fn undo_count(d: &Driven<AlignmentManagerDialog>) -> usize {
    d.document().history().undo_count()
}

mod alignment_manager {
    use super::*;

    #[test]
    fn new_from_the_selected_polyline_runs_align_new_with_its_vertices_as_one_step() {
        let mut document = Document::new(None);
        let line = Polyline2 {
            vertices: vec![
                Point2::new(0.0, 0.0),
                Point2::new(100.25, 0.0),
                Point2::new(100.25, 80.0),
            ],
            closed: false,
        };
        let attributes = document.current_attributes();
        document
            .execute(create_polyline(line, attributes))
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        let id = *document
            .model()
            .entities
            .ids()
            .last()
            .expect("the polyline");
        document.selection_mut().set(vec![id]);
        document.notify_selection_changed();

        let mut d = manager(interpreter_host(document));
        assert_eq!(d.rows("alignmentList"), 0);
        // Nothing to act on yet: the buttons that need an alignment say so.
        assert!(!d.enabled("alignmentDelete"));

        d.fill("alignmentName", "main road");
        d.press("alignmentUseSelection");
        assert_eq!(d.text("alignmentPoints"), "0,0 100.25,0 100.25,80");
        let before = undo_count(&d);
        d.press("alignmentNew");
        // A name with a blank is quoted, as a person would have to type it.
        assert_eq!(d.last(), "ALIGN NEW \"main road\" 0,0 100.25,0 100.25,80");
        {
            let alignments = &d.document().model().alignments;
            let created = alignments.find("main road").expect("main road");
            assert_eq!(created.horizontal.pis.len(), 3);
        }
        assert_eq!(d.dialog.current_alignment(), "main road");
        assert_eq!(undo_count(&d), before + 1);
        assert!(d.enabled("alignmentDelete"));

        d.document_mut()
            .undo()
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(d.document().model().alignments.find("main road").is_none());
    }

    #[test]
    fn use_selection_refuses_anything_but_one_line_or_polyline() {
        let mut d = manager(interpreter_host(Document::new(None)));
        d.press("alignmentUseSelection");
        let said = status(&d);
        assert!(said.contains("Select one line or polyline"), "{said}");
        assert!(d.text("alignmentPoints").is_empty());
        d.press("alignmentNew");
        assert!(d.lines().is_empty()); // no name: nothing is run
    }

    #[test]
    fn the_list_shows_every_alignment_and_reloads_when_one_is_typed_elsewhere() {
        let mut d = fixture();
        assert_eq!(d.rows("alignmentList"), 1);
        assert_eq!(cell_text(&d, "alignmentList", 0, 0), "road");
        assert_eq!(cell_text(&d, "alignmentList", 0, 1), "3");
        assert_eq!(cell_text(&d, "alignmentList", 0, 4), "200.000");
        assert_eq!(cell_text(&d, "alignmentList", 0, 5), "none");
        assert_eq!(d.dialog.current_alignment(), "road");

        // Typed on the command line, not in the dialog: the watcher reloads it, keeping the
        // chosen alignment.
        d.ok("ALIGN NEW access 0,0 10,0");
        d.settle();
        assert_eq!(d.rows("alignmentList"), 2);
        assert_eq!(d.dialog.current_alignment(), "road");
        {
            let Driven { dialog, host, .. } = &mut d;
            assert!(dialog.select_alignment(host.document(), "access"));
        }
        assert_eq!(row_count(&d, "alignmentPiTable"), 2);
        let Driven { dialog, host, .. } = &mut d;
        assert!(!dialog.select_alignment(host.document(), "nosuch"));
    }

    #[test]
    fn a_grid_edit_runs_nothing_until_apply_runs_one_pis_line_as_one_undo_step() {
        let mut d = fixture();
        assert_eq!(row_count(&d, "alignmentPiTable"), 3);
        assert_eq!(cell_text(&d, "alignmentPiTable", 2, 0), "2");

        edit(&mut d, "alignmentPiTable", 1, 3, "50");
        // Nothing runs from the table's own change.
        assert!(d.lines().is_empty());
        assert_eq!(road(&d).expect("road").horizontal.pis[1].radius, 0.0);

        let before = undo_count(&d);
        d.press("alignmentApplyPis");
        // A corner with no curve is written as x,y alone.
        assert_eq!(d.last(), "ALIGN PIS road 0,0 100,0,50 100,100");
        assert_eq!(road(&d).expect("road").horizontal.pis[1].radius, 50.0);
        assert_eq!(undo_count(&d), before + 1);
        // R = 50 round a right angle: 100 + 50 pi / 2 = 178.540 long.
        assert_eq!(cell_text(&d, "alignmentList", 0, 4), "178.540");

        d.document_mut()
            .undo()
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(road(&d).expect("road").horizontal.pis[1].radius, 0.0);
        d.settle();
        assert_eq!(cell_text(&d, "alignmentPiTable", 1, 3), "0");
    }

    #[test]
    fn an_edited_easting_is_written_and_every_other_number_goes_back_exactly() {
        let mut host = interpreter_host(Document::new(None));
        ok(&mut host, "ALIGN NEW road 0,0 100,0.123456789012 200,300");
        let mut d = manager(host);
        assert_eq!(cell_text(&d, "alignmentPiTable", 1, 2), "0.123456789012");
        edit(&mut d, "alignmentPiTable", 1, 1, "101");
        d.press("alignmentApplyPis");
        assert_eq!(d.last(), "ALIGN PIS road 0,0 101,0.123456789012 200,300");
        assert_eq!(
            road(&d).expect("road").horizontal.pis[1].point,
            Point2::new(101.0, 0.123456789012)
        );
    }

    #[test]
    fn a_refused_apply_keeps_the_grid_for_correcting_and_revert_puts_the_drawings_back() {
        let mut d = fixture();
        // A spiral cannot be negative (geometry::solve_alignment).
        edit(&mut d, "alignmentPiTable", 1, 4, "-5");
        d.press("alignmentApplyPis");
        assert_eq!(d.last(), "ALIGN PIS road 0,0 100,0,0,-5 100,100");
        let said = status(&d);
        assert!(said.contains("spiral"), "{said}");
        assert_eq!(road(&d).expect("road").horizontal.pis[1].spiral_in, 0.0);
        assert_eq!(cell_text(&d, "alignmentPiTable", 1, 4), "-5");
        d.press("alignmentRevertPis");
        assert_eq!(cell_text(&d, "alignmentPiTable", 1, 4), "0");
    }

    #[test]
    fn rows_are_added_and_removed_in_the_grid_and_the_start_chainage_has_its_own_line() {
        let mut d = fixture();
        d.press("alignmentAddPi");
        assert_eq!(row_count(&d, "alignmentPiTable"), 4);
        assert_eq!(cell_text(&d, "alignmentPiTable", 3, 0), "3");
        assert!(d.dialog.edit_cell("alignmentPiTable", 3, 1, "200"));
        assert!(d.dialog.edit_cell("alignmentPiTable", 3, 2, "100"));
        d.press("alignmentApplyPis");
        assert_eq!(d.last(), "ALIGN PIS road 0,0 100,0 100,100 200,100");
        assert_eq!(road(&d).expect("road").horizontal.pis.len(), 4);

        assert!(d.dialog.set_current_cell("alignmentPiTable", 1));
        d.press("alignmentRemovePi");
        // The rows below are counted again, as the PIs will be.
        assert_eq!(cell_text(&d, "alignmentPiTable", 1, 0), "1");
        d.press("alignmentApplyPis");
        assert_eq!(d.last(), "ALIGN PIS road 0,0 100,100 200,100");
        assert_eq!(road(&d).expect("road").horizontal.pis.len(), 3);

        // A row with a northing and no easting is refused before anything runs.
        let ran = d.lines().len();
        d.press("alignmentAddPi");
        assert!(d.dialog.edit_cell("alignmentPiTable", 3, 2, "5"));
        d.press("alignmentApplyPis");
        assert_eq!(d.lines().len(), ran);
        assert_eq!(status(&d), "PI 3 needs an easting and a northing.");
        d.press("alignmentRevertPis");

        d.fill("alignmentStartStation", "1000");
        d.press("alignmentStart");
        assert_eq!(d.last(), "ALIGN START road 1000");
        assert_eq!(road(&d).expect("road").horizontal.start_station, 1000.0);
        assert_eq!(cell_text(&d, "alignmentList", 0, 2), "1000.000");
    }

    #[test]
    fn unapplied_pi_edits_survive_a_reload_that_left_the_p_is_alone() {
        let mut d = fixture();
        edit(&mut d, "alignmentPiTable", 2, 2, "150");
        // The profile is not the PIs: a design typed elsewhere keeps the edit.
        d.ok("ALIGN DESIGN road 0,16 200,17");
        d.settle();
        assert_eq!(cell_text(&d, "alignmentPiTable", 2, 2), "150");
        // The PIs changed under it: the grid is the drawing's again.
        d.ok("ALIGN PI road 200,100");
        d.settle();
        assert_eq!(row_count(&d, "alignmentPiTable"), 4);
        assert_eq!(cell_text(&d, "alignmentPiTable", 2, 2), "100");
    }

    #[test]
    fn the_profile_grid_is_applied_as_one_design_line_and_its_elements_are_shown() {
        let mut d = fixture();
        d.fill("alignmentTabs", "Vertical");
        assert_eq!(row_count(&d, "alignmentPviTable"), 0);
        assert!(d.text("alignmentHighLow").starts_with("No design profile"));
        // The textbook sag: -3% into +2% over 100 m.
        for row in [["0", "16", ""], ["100", "13", "100"], ["300", "17", "0"]] {
            d.press("alignmentAddPvi");
            let at = row_count(&d, "alignmentPviTable") - 1;
            for (column, text) in row.iter().enumerate() {
                assert!(d.dialog.edit_cell("alignmentPviTable", at, column, text));
            }
        }
        let before = undo_count(&d);
        d.press("alignmentApplyProfile");
        assert_eq!(d.last(), "ALIGN DESIGN road 0,16 100,13,100 300,17");
        let profile = road(&d).expect("road").vertical.expect("a design profile");
        assert_eq!(profile.pvis.len(), 3);
        assert_eq!(undo_count(&d), before + 1);

        // Tangent to 50, the curve 50 to 150 with K = 100 / 5 = 20, tangent on; the low point
        // where the grade passes through level, 110 @ 13.6.
        assert_eq!(row_count(&d, "alignmentProfileElements"), 3);
        assert_eq!(cell_text(&d, "alignmentProfileElements", 1, 0), "curve");
        assert_eq!(cell_text(&d, "alignmentProfileElements", 1, 3), "-3.000");
        assert_eq!(cell_text(&d, "alignmentProfileElements", 1, 4), "2.000");
        assert_eq!(cell_text(&d, "alignmentProfileElements", 1, 5), "20.000");
        assert!(cell_text(&d, "alignmentProfileElements", 0, 5).is_empty());
        assert_eq!(
            d.text("alignmentHighLow"),
            "Low point at chainage 110.000, level 13.600."
        );
        assert_eq!(cell_text(&d, "alignmentList", 0, 5), "3 PVIs");

        d.press("alignmentClearProfile");
        assert_eq!(d.last(), "ALIGN CLEARPROFILE road");
        assert!(road(&d).expect("road").vertical.is_none());
        assert_eq!(row_count(&d, "alignmentPviTable"), 0);
    }

    #[test]
    fn an_incomplete_profile_row_is_refused_before_anything_runs() {
        let mut d = fixture();
        d.fill("alignmentTabs", "Vertical");
        d.press("alignmentAddPvi");
        d.press("alignmentAddPvi");
        assert!(d.dialog.edit_cell("alignmentPviTable", 0, 0, "0"));
        assert!(d.dialog.edit_cell("alignmentPviTable", 0, 1, "16"));
        assert!(d.dialog.edit_cell("alignmentPviTable", 1, 0, "100"));
        d.press("alignmentApplyProfile");
        assert!(d.lines().is_empty());
        assert_eq!(status(&d), "Profile row 2 needs a chainage and a level.");
    }

    // A cell is one number. A decimal comma, or an x,y pasted into one cell, was once glued into
    // the line as it stood: "100,5" in the easting made the PI x=100 y=5 and the northing its
    // radius, and the verb took it without a word.
    #[test]
    fn a_cell_that_is_not_one_number_is_refused_naming_it_before_anything_runs() {
        let mut d = fixture();
        edit(&mut d, "alignmentPiTable", 1, 1, "100,5");
        d.press("alignmentApplyPis");
        assert!(d.lines().is_empty());
        assert_eq!(
            status(&d),
            "PI 1: the Easting '100,5' is not a number; type one number, with a decimal point"
        );
        assert_eq!(
            road(&d).expect("road").horizontal.pis[1].point,
            Point2::new(100.0, 0.0)
        );
        d.press("alignmentRevertPis");
        edit(&mut d, "alignmentPiTable", 1, 3, "150,5");
        d.press("alignmentApplyPis");
        assert!(d.lines().is_empty());
        assert_eq!(
            status(&d),
            "PI 1: the Radius '150,5' is not a number; type one number, with a decimal point"
        );
        // A number in another spelling is written exactly, as the verb reads it.
        edit(&mut d, "alignmentPiTable", 1, 3, " 5e1 ");
        d.press("alignmentApplyPis");
        assert_eq!(d.last(), "ALIGN PIS road 0,0 100,0,50 100,100");

        d.fill("alignmentTabs", "Vertical");
        d.press("alignmentAddPvi");
        d.press("alignmentAddPvi");
        assert!(d.dialog.edit_cell("alignmentPviTable", 0, 0, "0,5"));
        assert!(d.dialog.edit_cell("alignmentPviTable", 0, 1, "10"));
        assert!(d.dialog.edit_cell("alignmentPviTable", 1, 0, "100"));
        assert!(d.dialog.edit_cell("alignmentPviTable", 1, 1, "12"));
        let ran = d.lines().len();
        d.press("alignmentApplyProfile");
        assert_eq!(d.lines().len(), ran);
        assert_eq!(
            status(&d),
            "Profile row 1: the Chainage '0,5' is not a number; type one number, with a \
             decimal point"
        );
        assert!(road(&d).expect("road").vertical.is_none());
    }

    #[test]
    fn unapplied_profile_edits_survive_another_edits_reload() {
        let mut d = fixture();
        d.ok("ALIGN DESIGN road 0,16 300,17");
        d.settle();
        assert_eq!(row_count(&d, "alignmentPviTable"), 2);
        assert!(d.dialog.edit_cell("alignmentPviTable", 1, 1, "18"));
        // A new start chainage reloads the dialog; the profile it did not touch keeps the grid's
        // unapplied level.
        d.fill("alignmentStartStation", "10");
        d.press("alignmentStart");
        assert_eq!(road(&d).expect("road").horizontal.start_station, 10.0);
        assert_eq!(cell_text(&d, "alignmentPviTable", 1, 1), "18");
        // A change to the profile itself refills the grid from the model.
        d.ok("ALIGN PVI road 400 20");
        d.settle();
        assert_eq!(row_count(&d, "alignmentPviTable"), 3);
        assert_eq!(cell_text(&d, "alignmentPviTable", 1, 1), "17");
    }

    #[test]
    fn the_setting_out_table_is_the_setting_out_functions_with_the_key_stations() {
        let mut d = fixture();
        d.ok("ALIGN SET road 1 50");
        d.settle();
        d.fill("alignmentTabs", "Setting Out");
        d.fill("alignmentInterval", "25");

        let solved = solve_alignment(&road(&d).expect("road").horizontal)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        let rows = setting_out_stations(&solved, 25.0)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(row_count(&d, "alignmentStations"), rows.len());
        assert_eq!(row_count(&d, "alignmentStations"), 10);
        assert_eq!(
            d.text("alignmentStationsNote"),
            "10 stations, 4 of them key stations."
        );
        // The TC at 50 is on the interval and is one row; the CT at 128.540 is not, and is there
        // all the same.
        assert_eq!(cell_text(&d, "alignmentStations", 2, 0), "50.000");
        assert_eq!(cell_text(&d, "alignmentStations", 2, 5), "TC");
        assert_eq!(cell_text(&d, "alignmentStations", 6, 0), "128.540");
        assert_eq!(cell_text(&d, "alignmentStations", 6, 5), "CT");
        assert_eq!(cell_text(&d, "alignmentStations", 0, 3), "90\u{b0}00'00\"");
        assert_eq!(cell_text(&d, "alignmentStations", 3, 4), "50.000 L");
        assert_eq!(cell_text(&d, "alignmentStations", 1, 4), "straight");

        let folder = Scratch::new("alignment-setting-out");
        let written_to = folder.path("road.csv");
        let path = written_to.as_path();
        d.dialog
            .save_stations_csv(d.document(), path)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        let written = std::fs::read_to_string(path).expect("the file");
        assert_eq!(written, setting_out_csv(&rows));

        d.press("alignmentStationsCopy");
        let copied = d.host.parts.clipboard.text().expect("the table was copied");
        assert!(
            copied.starts_with("Chainage\tEasting\tNorthing\tAzimuth\tRadius\tKey\n"),
            "{copied}"
        );
        assert_eq!(copied.matches('\n').count(), 11);
    }

    #[test]
    fn an_interval_that_cannot_be_used_empties_the_table_and_says_why() {
        let mut d = fixture();
        d.fill("alignmentTabs", "Setting Out");
        d.fill("alignmentInterval", "0");
        assert_eq!(row_count(&d, "alignmentStations"), 0);
        let note = d.text("alignmentStationsNote");
        assert!(note.contains("interval must be positive"), "{note}");
        // The line's own status is not where a half-typed interval is told off.
        assert!(status(&d).is_empty(), "{}", status(&d));
        let folder = Scratch::new("alignment-never");
        assert!(
            d.dialog
                .save_stations_csv(d.document(), &folder.path("never.csv"))
                .is_err()
        );
    }

    #[test]
    fn save_csv_in_a_headless_session_opens_no_file_dialog() {
        let mut host = interpreter_host(Document::new(None));
        ok(&mut host, "ALIGN NEW road 0,0 100,0");
        host.parts.headless = true;
        let mut d = manager(host);
        d.fill("alignmentTabs", "Setting Out");
        d.press("alignmentStationsCsv");
        let said = status(&d);
        assert!(said.contains("headless"), "{said}");
    }

    #[test]
    fn chainage_labels_need_a_chainage_style_and_are_one_step() {
        let mut d = fixture();
        d.fill("alignmentTabs", "Setting Out");
        assert!(d.control("alignmentLabelStyle").items.is_empty());
        d.press("alignmentLabelChainages");
        assert!(d.lines().is_empty());
        let said = status(&d);
        assert!(said.contains("LABELSTYLE DEFAULTS"), "{said}");

        d.ok("LABELSTYLE DEFAULTS");
        d.settle();
        // Only the styles that label chainages are offered.
        assert_eq!(d.control("alignmentLabelStyle").items, ["Chainage"]);

        d.ok("LAYER NEW chainage");
        let entities = d.document().model().entities.size();
        let before = undo_count(&d);
        d.fill("alignmentLabelLayer", "chainage");
        d.press("alignmentLabelChainages");
        assert_eq!(d.last(), "LABEL ALIGN road style=Chainage layer=chainage");
        assert!(d.document().model().entities.size() > entities);
        assert_eq!(undo_count(&d), before + 1);
    }

    #[test]
    fn delete_runs_align_delete_and_leaves_nothing_to_edit() {
        let mut d = fixture();
        d.press("alignmentDelete");
        assert_eq!(d.last(), "ALIGN DELETE road");
        assert!(road(&d).is_none());
        assert_eq!(d.rows("alignmentList"), 0);
        assert_eq!(row_count(&d, "alignmentPiTable"), 0);
        assert!(!d.enabled("alignmentApplyPis"));
        d.fill("alignmentTabs", "Setting Out");
        assert!(!d.enabled("alignmentLabelChainages"));
    }

    #[test]
    fn a_dialog_with_no_executor_says_so_and_changes_nothing() {
        let mut host = RecordingHost::new();
        ok(&mut host, "ALIGN NEW road 0,0 100,0");
        let dialog = AlignmentManagerDialog::new(host.document()).without_executor();
        let mut d = Driven::new(host, dialog, "alignmentManagerDialog");
        d.press("alignmentDelete");
        let said = status(&d);
        assert!(said.starts_with("Nothing here can run"), "{said}");
        assert!(road(&d).is_some());
    }

    #[test]
    fn a_drawing_that_goes_first_leaves_the_dialog_inert() {
        let mut host = RecordingHost::new().answering(Answer::Fixed(
            katana_window::host::VerbOutcome::new(true, "", ""),
        ));
        ok(&mut host, "ALIGN NEW road 0,0 100,0");
        let mut d = manager(host);
        // The drawing the dialog was made with goes, and another takes its place in the host.
        d.host.parts.document = Document::new(None);
        d.settle();
        d.press("alignmentDelete");
        assert!(d.lines().is_empty());
        let said = status(&d);
        assert!(said.contains("gone"), "{said}");
    }
}
