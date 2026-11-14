//! The twins of tests/qt_widgets/terrain/test_volumes_dialog.cpp: Terrain > Volumes
//! (katana_window::terrain::volumes_dialog). Each page's line, built from its fields as a person
//! fills them - found by object name - and handed to the executor exactly as shown; and, with the
//! interpreter itself as the executor, the records it answers in the table and the mass haul
//! diagram, and the cut/fill map made as a surface of the drawing.
//!
//! What the C++ did to a control through Qt's own class is a driver's act here: a radio button's
//! click is a press, a box's setChecked a tick, a combo's setCurrentIndex(findData(word)) a fill of
//! the item's label, a spin box's setValue a fill of the number. The C++'s processEvents is a
//! frame of the harness, in which the dialog's DocumentWatcher delivers.

use std::fs;
use std::sync::Arc;

use katana_geometry::Point3;
use katana_terrain::{TinBuildOptions, TinInput, TinSurface, build_tin};
use katana_window::drive::Part;
use katana_window::host::RecordingHost;
use katana_window::terrain::volumes_dialog::{VolumeMethod, VolumesDialog, volume_result_table};

use super::support::{Bench, Drawn, interpreter_host, keep, named, set_surfaces};
use super::window_support::{Scratch, fixed_host, offscreen_harness, ok};

fn surface_of(points: &[Point3]) -> TinSurface {
    let input = TinInput {
        points: points.to_vec(),
        ..TinInput::default()
    };
    build_tin(&input, &TinBuildOptions::default())
        .unwrap_or_else(|error| panic!("{}", error.describe()))
        .surface
}

fn add_surface(host: &mut RecordingHost, name: &str, surface: TinSurface) {
    let mut store = host.document().surfaces().clone();
    keep(&mut store, named(name, Arc::new(surface), "test"));
    set_surfaces(host, store, "TEST");
}

/// A line run by the host's interpreter on its drawing, as the C++ tests ran it on theirs - not
/// through the dialog's executor, so the host does not record it.
fn run(host: &mut RecordingHost, line: &str) {
    let parts = &mut host.parts;
    parts
        .interpreter
        .run(&mut parts.document, line)
        .unwrap_or_else(|error| panic!("{line}: {}", error.describe()));
}

/// The wedge of docs/volumes.md: the ground z = 0 under the triangle (0,0), (100,-10), (100,10) of
/// z = 0.1 x, and the alignment cl along it. A section at s holds s^2 / 50: 6700 by end areas
/// every 10 m.
fn add_wedge(host: &mut RecordingHost) {
    add_surface(
        host,
        "ground",
        surface_of(&[
            Point3::new(-20.0, -60.0, 0.0),
            Point3::new(120.0, -60.0, 0.0),
            Point3::new(120.0, 80.0, 0.0),
            Point3::new(-20.0, 80.0, 0.0),
        ]),
    );
    add_surface(
        host,
        "design",
        surface_of(&[
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(100.0, -10.0, 10.0),
            Point3::new(100.0, 10.0, 10.0),
        ]),
    );
    run(host, "ALIGN NEW cl 0,0 100,0");
}

/// The executor the C++ tests hand the dialog: a record of the lines, answered by the interpreter
/// on the test's drawing - the verbs the window runs. Headless, as the C++'s Runner said.
fn wedge_host() -> RecordingHost {
    let mut host = interpreter_host();
    add_wedge(&mut host);
    host
}

/// The dialog on `host`'s drawing, shown and drawn once.
fn bench(mut host: RecordingHost) -> Bench<VolumesDialog> {
    let mut dialog = VolumesDialog::new(&mut host);
    dialog.show(&host);
    Bench::new("volumesDialog", dialog, host)
}

/// `dialog.setMethod(method)`, and the frames that show the page.
fn set_method(bench: &mut Bench<VolumesDialog>, method: VolumeMethod) {
    bench.dialog.set_method(method);
    bench.draw();
}

fn command(bench: &Bench<VolumesDialog>) -> String {
    bench.text("volumesCommand")
}

/// EXPECT_DOUBLE_EQ: within four units in the last place, as gtest's AlmostEquals counts them.
fn double_eq(left: f64, right: f64) -> bool {
    const SIGN: u64 = 1 << 63;
    let biased = |value: f64| {
        let bits = value.to_bits();
        if bits & SIGN == 0 {
            bits | SIGN
        } else {
            (!bits).wrapping_add(1)
        }
    };
    let (a, b) = (biased(left), biased(right));
    a.abs_diff(b) <= 4
}

mod volumes_dialog {
    use super::*;

    #[test]
    fn each_page_says_the_line_its_fields_make() {
        let mut bench = bench(wedge_host());

        // Surface to Surface: the base and the top, and the top starts on the second surface, so
        // the two are not one surface against itself.
        set_method(&mut bench, VolumeMethod::Surfaces);
        assert_eq!(command(&bench), "VOLUME SURFACES ground design");
        bench.press("volumesBoundaryCorners");
        bench.fill("volumesCorners", "0,0 10,0 10,10");
        bench.tick("volumesEach", true);
        assert_eq!(
            command(&bench),
            "VOLUME SURFACES ground design INSIDE 0,0 10,0 10,10 EACH"
        );
        bench.press("volumesBoundaryScope");
        bench.press("volumesScopeDrawing");
        assert_eq!(
            command(&bench),
            "VOLUME SURFACES ground design INSIDE FROM DRAWING EACH"
        );
        bench.fill("volumesBulk", "1.25");
        assert_eq!(
            command(&bench),
            "VOLUME SURFACES ground design INSIDE FROM DRAWING EACH bulk=1.25"
        );
        bench.fill("volumesBulk", "1");
        bench.press("volumesBoundaryNone");

        set_method(&mut bench, VolumeMethod::Level);
        bench.fill("volumesLevel", "12.5");
        assert_eq!(command(&bench), "VOLUME DATUM ground 12.5");

        set_method(&mut bench, VolumeMethod::Stockpile);
        bench.press("volumesBoundaryCorners");
        bench.tick("volumesEach", false);
        bench.fill("volumesStockpileBase", "The lowest point on the boundary");
        assert_eq!(
            command(&bench),
            "VOLUME STOCKPILE ground INSIDE 0,0 10,0 10,10 base=lowest"
        );
        bench.fill("volumesStockpileBase", "A level");
        bench.fill("volumesStockpileLevel", "-2");
        assert_eq!(
            command(&bench),
            "VOLUME STOCKPILE ground INSIDE 0,0 10,0 10,10 level=-2"
        );

        set_method(&mut bench, VolumeMethod::Sections);
        assert_eq!(
            command(&bench),
            "VOLUME SECTIONS ground design ALIGNMENT cl interval=10"
        );
        bench.fill("volumesLeft", "20");
        bench.fill("volumesRight", "20");
        bench.fill(
            "volumesSectionMethod",
            "Both, with the prismoidal correction",
        );
        assert_eq!(
            command(&bench),
            "VOLUME SECTIONS ground design ALIGNMENT cl interval=10 left=20 right=20 method=both"
        );

        set_method(&mut bench, VolumeMethod::Grid);
        bench.fill("volumesCell", "2");
        assert_eq!(
            command(&bench),
            "VOLUME GRID ground design cell=2 INSIDE 0,0 10,0 10,10"
        );
        assert!(bench.host.lines().is_empty()); // building a line runs nothing
    }

    #[test]
    fn the_curvature_box_is_on_by_default_says_nothing_then_and_curvature_off_when_unchecked() {
        // The correction is the verb's default, so the line only names the plain figure; the box
        // is on the Along Alignment page alone, and the line it makes is the one handed to the
        // executor.
        let mut bench = bench(wedge_host());
        set_method(&mut bench, VolumeMethod::Sections);
        assert_eq!(bench.value("volumesCurvature"), "on");
        assert_eq!(
            command(&bench),
            "VOLUME SECTIONS ground design ALIGNMENT cl interval=10"
        );
        bench.tick("volumesCurvature", false);
        assert_eq!(
            command(&bench),
            "VOLUME SECTIONS ground design ALIGNMENT cl interval=10 curvature=off"
        );
        bench.fill(
            "volumesSectionMethod",
            "Both, with the prismoidal correction",
        );
        assert_eq!(
            command(&bench),
            "VOLUME SECTIONS ground design ALIGNMENT cl interval=10 method=both curvature=off"
        );
        bench.fill("volumesSectionMethod", "Average end area");
        // The other pages take no curvature=, and say nothing of it.
        set_method(&mut bench, VolumeMethod::Surfaces);
        assert_eq!(command(&bench), "VOLUME SURFACES ground design");
        set_method(&mut bench, VolumeMethod::Sections);

        // Measure runs the line as shown, and the reply's volume record says the figure is the
        // plain one.
        bench.fill("volumesLeft", "20");
        bench.fill("volumesRight", "20");
        bench.press("volumesRun");
        assert_eq!(bench.host.lines().len(), 1);
        assert_eq!(
            bench.host.lines()[0],
            "VOLUME SECTIONS ground design ALIGNMENT cl interval=10 left=20 right=20 curvature=off"
        );
        assert!(bench.dialog.results_text().contains("\toff\t"));
        // Checked again, the word goes.
        bench.tick("volumesCurvature", true);
        assert_eq!(
            command(&bench),
            "VOLUME SECTIONS ground design ALIGNMENT cl interval=10 left=20 right=20"
        );
    }

    #[test]
    fn a_stockpile_with_no_boundary_is_refused_naming_what_is_missing_and_runs_nothing() {
        let mut bench = bench(wedge_host());
        set_method(&mut bench, VolumeMethod::Stockpile);
        bench.press("volumesBoundaryNone");
        assert!(command(&bench).is_empty());
        bench.press("volumesRun");
        assert!(bench.host.lines().is_empty());
        assert!(bench.text("volumesStatus").contains("boundary"));
    }

    #[test]
    fn measuring_along_an_alignment_fills_the_table_and_the_mass_haul_diagram() {
        let mut bench = bench(wedge_host());
        set_method(&mut bench, VolumeMethod::Sections);
        bench.fill("volumesLeft", "20");
        bench.fill("volumesRight", "20");
        bench.press("volumesRun");
        assert_eq!(bench.host.lines().len(), 1);
        assert_eq!(bench.host.lines()[0], command(&bench));

        // Eleven sections and the end-area total.
        assert_eq!(bench.items("volumesResults").len(), 12);
        // The diagram: every section's ordinate, falling to -6700 (all fill).
        let points = bench.dialog.chart().points();
        assert_eq!(points.len(), 11);
        assert!(double_eq(points[0].x, 0.0), "{}", points[0].x);
        assert!(double_eq(points[10].x, 100.0), "{}", points[10].x);
        assert!(double_eq(points[10].y, -6700.0), "{}", points[10].y);
        assert!(bench.text("volumesStatus").contains("6700.000"));
        // Copy's text is the table, its header first.
        assert!(bench.dialog.results_text().starts_with("record\tstation"));
    }

    #[test]
    fn make_cut_fill_map_adds_the_difference_and_offers_it_at_once() {
        let mut bench = bench(wedge_host());
        set_method(&mut bench, VolumeMethod::Surfaces);
        bench.fill("volumesMapName", "cut fill");
        bench.press("volumesCutFillMap");
        assert_eq!(bench.host.lines().len(), 1);
        assert_eq!(
            bench.host.lines()[0],
            "SURFACE DIFFERENCE ground design NAME \"cut fill\""
        );
        assert!(bench.host.document().surfaces().find("cut fill").is_some());
        assert!(
            bench
                .items("volumesBase")
                .iter()
                .any(|name| name == "cut fill")
        );
    }

    #[test]
    fn make_envelope_runs_surface_envelope_of_the_pages_two_surfaces_and_offers_the_result_at_once()
    {
        let mut bench = bench(wedge_host());
        assert!(bench.has("volumesEnvelopeSide"));

        // The line the fields make: the page's two surfaces, MAX by default.
        set_method(&mut bench, VolumeMethod::Surfaces);
        let upper = bench
            .dialog
            .envelope_line()
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(upper, "SURFACE ENVELOPE ground design MAX");
        bench.fill("volumesEnvelopeSide", "The lower of the two");
        bench.fill("volumesEnvelopeName", "lower level");
        let lower = bench
            .dialog
            .envelope_line()
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert_eq!(
            lower,
            "SURFACE ENVELOPE ground design MIN NAME \"lower level\""
        );
        // The other pages use their own two surfaces, as the cut/fill map does.
        set_method(&mut bench, VolumeMethod::Grid);
        assert_eq!(
            bench.dialog.envelope_line().unwrap(),
            "SURFACE ENVELOPE ground design MIN NAME \"lower level\""
        );
        set_method(&mut bench, VolumeMethod::Sections);
        assert_eq!(
            bench.dialog.envelope_line().unwrap(),
            "SURFACE ENVELOPE ground design MIN NAME \"lower level\""
        );
        set_method(&mut bench, VolumeMethod::Surfaces);
        assert!(bench.host.lines().is_empty()); // building a line runs nothing

        // The button hands exactly that line to the executor; the surface is in the drawing in
        // the default style and the lists offer it at once.
        bench.press("volumesEnvelope");
        assert_eq!(bench.host.lines().len(), 1);
        assert_eq!(
            bench.host.lines()[0],
            "SURFACE ENVELOPE ground design MIN NAME \"lower level\""
        );
        let made = bench
            .host
            .document()
            .surfaces()
            .find("lower level")
            .expect("the envelope is a surface of the drawing");
        assert!(made.style.is_empty());
        assert!(
            bench
                .items("volumesBase")
                .iter()
                .any(|name| name == "lower level")
        );
        assert!(bench.text("volumesStatus").contains("Made the envelope"));
        assert!(bench.text("volumesStatus").contains("lower"));

        // Without a name the verb's own: ground-design-max.
        bench.fill("volumesEnvelopeSide", "The higher of the two");
        bench.fill("volumesEnvelopeName", "");
        bench.press("volumesEnvelope");
        assert_eq!(bench.host.lines().len(), 2);
        assert_eq!(
            bench.host.lines()[1],
            "SURFACE ENVELOPE ground design MAX"
        );
        assert!(
            bench
                .host
                .document()
                .surfaces()
                .find("ground-design-max")
                .is_some()
        );
    }

    #[test]
    fn make_envelope_with_no_surface_or_a_refused_name_says_why_and_changes_nothing() {
        // No surfaces: a line cannot be made, naming what is missing, and nothing runs.
        {
            let mut bench = bench(interpreter_host());
            let none = bench.dialog.envelope_line();
            let error = none.expect_err("no surface, no line");
            assert!(error.message.contains("no surface"), "{}", error.message);
            bench.press("volumesEnvelope");
            assert!(bench.host.lines().is_empty());
            assert!(bench.text("volumesStatus").contains("no surface"));
        }
        // The verb's refusal reaches the status and the drawing is as it was: a name the drawing
        // already has.
        let mut bench = bench(wedge_host());
        bench.fill("volumesEnvelopeName", "ground");
        bench.press("volumesEnvelope");
        assert_eq!(bench.host.lines().len(), 1);
        assert!(bench.text("volumesStatus").contains("already has"));
        assert_eq!(bench.host.document().surfaces().all().len(), 2);
    }

    #[test]
    fn the_lists_follow_the_drawing_while_the_dialog_stays_open() {
        // The dialog is non-modal and kept. A surface made by a typed line, or one an UNDO
        // removes, used to be missed until the dialog was shown again: the line it built named a
        // surface the drawing no longer had (NotFound), and one made meanwhile could not be
        // chosen. Its own status said "UNDO removes it" and the removed map was still offered.
        let mut bench = bench(wedge_host());
        let offers = |bench: &Bench<VolumesDialog>, list: &str, name: &str| {
            bench.items(list).iter().any(|item| item == name)
        };
        assert_eq!(command(&bench), "VOLUME SURFACES ground design");
        assert_eq!(bench.items("volumesAlignment").len(), 1);

        // A surface made by a typed line is offered after a turn of the event loop.
        run(&mut bench.host, "SURFACE DIFFERENCE ground design NAME raised");
        bench.draw();
        assert!(offers(&bench, "volumesTop", "raised"));
        assert!(offers(&bench, "volumesBase", "raised"));
        bench.fill("volumesTop", "raised");
        assert_eq!(command(&bench), "VOLUME SURFACES ground raised");

        // UNDO removes it: not offered, and the line no longer names it.
        bench
            .host
            .document_mut()
            .undo()
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        bench.draw();
        assert!(!offers(&bench, "volumesTop", "raised"));
        assert!(!command(&bench).contains("raised"));

        // The alignment, and the surface the page had chosen, go the same way.
        bench
            .host
            .document_mut()
            .undo() // ALIGN NEW cl
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        bench.draw();
        assert_eq!(bench.items("volumesAlignment").len(), 0);
        bench
            .host
            .document_mut()
            .undo() // the design
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        bench.draw();
        assert!(!offers(&bench, "volumesTop", "design"));
        assert!(!command(&bench).contains("design"));
        assert!(bench.host.lines().is_empty()); // following the drawing runs nothing
    }

    #[test]
    fn save_csv_runs_the_same_line_writing_the_file_it_names() {
        let mut host = fixed_host(ok("volume method=exact cut=0.000"));
        add_wedge(&mut host);
        let mut bench = bench(host);
        set_method(&mut bench, VolumeMethod::Surfaces);
        // Headless, with no file named, nothing runs and the field is named.
        bench.press("volumesSaveCsv");
        assert!(bench.host.lines().is_empty());
        bench.fill("volumesCsvPath", "C:/site/volumes.csv");
        bench.press("volumesSaveCsv");
        assert_eq!(bench.host.lines().len(), 1);
        assert_eq!(
            bench.host.lines()[0],
            "VOLUME SURFACES ground design csv=C:/site/volumes.csv OVERWRITE"
        );
    }

    #[test]
    fn save_csv_of_a_scope_that_takes_no_polygon_says_nothing_was_saved_and_leaves_the_file() {
        // "The polygons below" on the default scope (the selection) with nothing selected: the
        // verb runs nothing and writes nothing, and the status used to say "Saved the table to
        // ..." over a file that still held what an earlier save left there.
        let mut bench = bench(wedge_host());
        set_method(&mut bench, VolumeMethod::Surfaces);
        let scratch = Scratch::new("volumes_dialog_csv");
        let file = scratch.file("katana_volumes_dialog.csv", "OLD TABLE FROM AN EARLIER RUN\r\n");
        // The C++'s generic_string: forward slashes on every system.
        let path = file.to_string_lossy().replace('\\', "/");
        bench.fill("volumesCsvPath", &path);
        bench.press("volumesBoundaryScope");

        bench.press("volumesSaveCsv");
        assert_eq!(bench.host.lines().len(), 1);
        let status = bench.text("volumesStatus");
        assert!(!status.contains("Saved the table"), "{status}");
        assert!(status.contains("was not written"), "{status}");
        assert!(bench.dialog.results_text().contains("written")); // the csv record is in the table
        let content = fs::read(&file).expect("the file is read");
        assert_eq!(content, b"OLD TABLE FROM AN EARLIER RUN\r\n");

        // The same save over typed corners that measure something writes the file and says so.
        bench.press("volumesBoundaryCorners");
        bench.fill("volumesCorners", "10,0 50,0 50,4");
        bench.press("volumesSaveCsv");
        assert_eq!(bench.host.lines().len(), 2);
        let status = bench.text("volumesStatus");
        assert!(status.contains("Saved the table to"), "{status}");
        let written = fs::read_to_string(&file).expect("the file is read");
        let first = written.split('\n').next().unwrap_or_default();
        assert!(
            first.starts_with("record,polygon,entity,polygon_area,area,cut,fill,net"),
            "{first}"
        );
    }

    // The tallest dialog the window has: every page's fields, the scope and filter controls, a
    // table and a diagram, a thousand pixels and more once shown. A laptop's screen has less room
    // than that, so it scrolls (shell::dialog's screen fit) and its Close button is still reached:
    // it is the dialog's footer, below the scroll area, so it is on the dialog as it stands and
    // does not move as the rest scrolls. Qt's screen is the offscreen platform's, 800 x 600; the
    // C++'s fitDialogToScreen is the frame's fitting at every frame.
    #[test]
    fn is_no_taller_than_the_screen_and_still_reaches_its_close_button() {
        let mut host = interpreter_host();
        let mut dialog = VolumesDialog::new(&mut host);
        dialog.show(&host);
        let mut harness = offscreen_harness();
        harness.target("volumesDialog");
        let mut bench = Bench {
            harness,
            dialog,
            host,
        };
        bench.draw();
        let available = bench.harness.screen();
        let target = bench
            .harness
            .target_record("volumesDialog")
            .expect("the dialog is drawn");
        let scroll = target.scroll.expect("the dialog is fitted to the screen");
        assert!(
            scroll.content.y > available.height() - 100.0,
            "the premise: the dialog needs more room than this screen has"
        );
        assert!(target.rect.height() <= available.height());
        let footer = target
            .footer
            .expect("the button box at the end of the layout is the footer");
        let close = bench.control("volumesClose");
        assert_eq!(close.part, Part::Footer);
        assert!(footer.contains_rect(close.rect));
        assert!(
            scroll.viewport.bottom() <= footer.top(),
            "Close scrolls with the rest"
        );
        assert!(close.visible);
        let resting = close.rect;
        assert!(
            target.rect.contains_rect(resting),
            "Close is off the dialog without scrolling"
        );
        // The body scrolled right down, and back: Close has not moved.
        for name in ["volumesReply", "volumesMethods"] {
            {
                let Bench {
                    harness,
                    dialog,
                    host,
                } = &mut bench;
                harness
                    .target("volumesDialog")
                    .scroll_to(name)
                    .run(|ui| dialog.draw(ui, &mut *host))
                    .unwrap_or_else(|problem| panic!("{problem}"));
            }
            assert_eq!(bench.control("volumesClose").rect, resting, "scrolled to {name}");
        }
    }
}

mod volume_result_table {
    use super::*;

    #[test]
    fn every_record_is_a_row_and_every_key_a_column_in_the_order_first_met() {
        let table = volume_result_table(
            "polygon index=1 cut=1.000 fill=2.000\n\
             polygon index=2 cut=3.000 fill=4.000 chorded=yes\n\
             volume method=exact cut=4.000 fill=6.000\n",
        );
        assert_eq!(
            table.columns,
            ["record", "index", "cut", "fill", "chorded", "method"]
        );
        assert_eq!(table.rows.len(), 3);
        assert_eq!(table.rows[1], ["polygon", "2", "3.000", "4.000", "yes", ""]);
        assert_eq!(table.rows[2], ["volume", "", "4.000", "6.000", "", "exact"]);
    }
}
