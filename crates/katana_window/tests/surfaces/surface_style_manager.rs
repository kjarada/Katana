//! The twins of tests/qt_widgets/terrain/test_surface_style_manager.cpp: Terrain > Surface Styles
//! (katana_window::terrain::surface_style_manager), driven by its object names as a person drives
//! it. The dialog changes nothing itself: it writes the SURFACESTYLE (or SURFACE STYLE) line a
//! person would type and hands it to the window's executor - so what is tested is each line, word
//! for word, and, through the real interpreter, that the drawing then holds what the line said.
//!
//! What the C++ asked of the dialog with no argument (`applyLine()`, `formStyle()`,
//! `selectStyle(name)`) is asked here with the host's drawing, which the dialog is lent and does
//! not keep. The preview the C++ grabbed from its widget is `preview_image` at the same size.

use std::sync::Arc;

use katana_cad::terrain::surface_style::{
    ColourStop, MAXIMUM_SURFACE_BANDS, SurfaceContours, SurfaceDisplayStyle, SurfaceFaces,
    SurfaceStyle, SurfaceStyleSet, check_surface_style,
};
use katana_cad::{CommandInterpreter, Document};
use katana_entity::Color;
use katana_geometry::Point3;
use katana_terrain::{SurfaceStore, TinSurface};
use katana_window::canvas::Color as Pixel;
use katana_window::host::RecordingHost;
use katana_window::terrain::surface_style_manager::{
    SurfaceStyleManagerDialog, surface_style_bands_range, surface_style_set_words,
};

use super::support::{Bench, Drawn, interpreter_host, keep, named, set_surfaces};
use super::window_support::{fixed_host, ok};

/// The drawing's own two styles, "Style A" the default - so nothing here depends on what the
/// built-in styles say - and a surface to assign.
fn prepare(host: &mut RecordingHost) {
    let a = SurfaceDisplayStyle {
        name: String::from("Style A"),
        contours: SurfaceContours {
            show: true,
            interval: 1.0,
            label_major: false,
            ..SurfaceContours::default()
        },
        ..SurfaceDisplayStyle::default()
    };
    let b = SurfaceDisplayStyle {
        name: String::from("Style B"),
        plan_faces: SurfaceFaces::Elevation,
        ..SurfaceDisplayStyle::default()
    };
    let set = SurfaceStyleSet {
        styles: vec![a, b],
        default_style: String::from("Style A"),
    };
    host.document_mut()
        .set_surface_styles(set, "TEST")
        .unwrap_or_else(|error| panic!("{}", error.describe()));
    let tin = TinSurface::create(
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(10.0, 0.0, 10.0),
            Point3::new(10.0, 10.0, 10.0),
            Point3::new(0.0, 10.0, 0.0),
        ],
        vec![[0, 1, 2], [0, 2, 3]],
        Vec::new(),
    )
    .unwrap_or_else(|error| panic!("{}", error.describe()));
    let mut store = SurfaceStore::new();
    keep(&mut store, named("ground", Arc::new(tin), "test"));
    set_surfaces(host, store, "TEST");
}

/// The dialog on `host`'s drawing, shown.
fn bench(host: RecordingHost) -> Bench<SurfaceStyleManagerDialog> {
    let mut dialog = SurfaceStyleManagerDialog::new(&host);
    dialog.show();
    Bench::new("surfaceStyleManagerDialog", dialog, host)
}

/// The dialog with a runner that only writes each line down, answering "done".
fn recorded() -> Bench<SurfaceStyleManagerDialog> {
    let mut host = fixed_host(ok("done"));
    prepare(&mut host);
    bench(host)
}

/// The dialog with the interpreter on the host's drawing as its runner.
fn interpreted() -> Bench<SurfaceStyleManagerDialog> {
    let mut host = interpreter_host();
    prepare(&mut host);
    bench(host)
}

/// `selectStyle(name)`, and the frames that show what it chose.
fn select(bench: &mut Bench<SurfaceStyleManagerDialog>, name: &str) -> bool {
    let found = bench.dialog.select_style(name, bench.host.document());
    bench.draw();
    found
}

fn apply_line(bench: &Bench<SurfaceStyleManagerDialog>) -> String {
    bench.dialog.apply_line(bench.host.document())
}

fn style<'a>(document: &'a Document, name: &str) -> &'a SurfaceDisplayStyle {
    document
        .surface_styles()
        .find(name)
        .unwrap_or_else(|| panic!("no style {name}"))
}

mod surface_style_manager {
    use super::*;

    #[test]
    fn it_lists_the_drawings_styles_and_names_the_default() {
        let bench = recorded();
        let list = bench.items("surfaceStyleList");
        assert_eq!(list.len(), 2);
        assert_eq!(list[0], "Style A");
        assert_eq!(list[1], "Style B");
        // The default is chosen first, and said.
        assert_eq!(bench.dialog.current_style(), "Style A");
        assert!(bench.text("surfaceStyleDefaultNote").contains("Style A"));
        let surfaces = bench.items("surfaceStyleSurface");
        assert_eq!(surfaces.len(), 1);
        assert_eq!(surfaces[0], "ground");
    }

    #[test]
    fn apply_runs_one_set_line_of_only_what_the_form_changed() {
        let mut bench = recorded();
        assert!(select(&mut bench, "Style A"));
        // Untouched, there is nothing to apply.
        assert!(apply_line(&bench).is_empty());
        assert!(!bench.enabled("surfaceStyleApply"));
        bench.fill("surfaceStyleInterval", "0.5");
        bench.tick("surfaceStyleLabels", true);
        bench.fill("surfaceStyleFaces", "slope");
        // In the verb's key order: faces, then the contours' interval and labels.
        let expected = "SURFACESTYLE SET \"Style A\" faces=slope interval=0.5 labels=on";
        assert_eq!(apply_line(&bench), expected);
        assert_eq!(bench.text("surfaceStyleLine"), expected);
        bench.press("surfaceStyleApply");
        assert_eq!(bench.host.lines().len(), 1);
        assert_eq!(bench.host.lines()[0], expected);
        assert_eq!(bench.text("surfaceStyleStatus"), "done");
    }

    #[test]
    fn each_button_builds_its_own_line() {
        let mut bench = recorded();
        bench.fill("surfaceStyleName", "My Style");
        assert!(select(&mut bench, "Style A"));
        bench.press("surfaceStyleNew");
        assert!(select(&mut bench, "Style A"));
        bench.press("surfaceStyleCopy");
        assert!(select(&mut bench, "Style A"));
        bench.press("surfaceStyleRename");
        assert!(select(&mut bench, "Style B"));
        bench.press("surfaceStyleSetDefault");
        assert!(select(&mut bench, "Style B"));
        bench.press("surfaceStyleDelete");
        assert!(select(&mut bench, "Style B"));
        bench.press("surfaceStyleAssign");
        let expected = [
            "SURFACESTYLE NEW \"My Style\"",
            "SURFACESTYLE NEW \"My Style\" FROM \"Style A\"",
            "SURFACESTYLE RENAME \"Style A\" \"My Style\"",
            "SURFACESTYLE DEFAULT \"Style B\"",
            "SURFACESTYLE DELETE \"Style B\"",
            "SURFACE STYLE ground \"Style B\"",
        ];
        assert_eq!(bench.host.lines(), expected);
    }

    #[test]
    fn a_field_that_does_not_read_stops_apply_and_says_why() {
        let mut bench = recorded();
        bench.fill("surfaceStyleInterval", "half a metre");
        assert!(!bench.enabled("surfaceStyleApply"));
        assert!(bench.text("surfaceStyleLine").starts_with("Cannot apply"));
        bench.fill("surfaceStyleInterval", "1");
        bench.fill("surfaceStyleTriangleColour", "grey-ish");
        assert!(!bench.enabled("surfaceStyleApply"));
        bench.fill("surfaceStyleTriangleColour", "#808080");
        assert!(bench.enabled("surfaceStyleApply"));
        // New with no name is not offered.
        bench.fill("surfaceStyleName", "  ");
        assert!(!bench.enabled("surfaceStyleNew"));
    }

    #[test]
    fn the_preview_draws_the_forms_style_before_it_is_applied() {
        let mut bench = recorded();
        assert!(select(&mut bench, "Style A"));
        assert!(bench.has("surfaceStylePreview"));
        let contours = bench.dialog.preview_image(320, 220);
        // The baseline the change is measured against: the sample surface's own footprint in this
        // preview, counted as the pixels opaque flat faces in a colour drawn nowhere else cover.
        // (It was a quarter of the whole preview, a guess: the sample covers less than that at
        // this size.)
        let opacity_as_shown = bench.value("surfaceStyleOpacity");
        bench.fill("surfaceStyleFlatColour", "#FF00FF");
        bench.fill("surfaceStyleOpacity", "1");
        bench.fill("surfaceStyleFaces", "flat");
        let flat = bench.dialog.preview_image(320, 220);
        bench.fill("surfaceStyleOpacity", &opacity_as_shown);
        bench.fill("surfaceStyleFaces", "elevation");
        let faces = bench.dialog.preview_image(320, 220);
        assert_eq!(
            (contours.width(), contours.height()),
            (faces.width(), faces.height())
        );
        assert_eq!((flat.width(), flat.height()), (faces.width(), faces.height()));
        let mut footprint = 0_usize;
        let mut changed = 0_usize;
        for y in 0..faces.height() {
            for x in 0..faces.width() {
                footprint += usize::from(flat.pixel(x, y) == Pixel::rgb(255, 0, 255));
                changed += usize::from(contours.pixel(x, y) != faces.pixel(x, y));
            }
        }
        assert!(footprint > 0, "the sample was not drawn");
        // Faces fill the sample where only lines were, so most of its footprint changes; what does
        // not is under the contour lines, drawn over the faces alike in both - a few pixels' width
        // along each, far less than half of the footprint.
        assert!(
            changed > footprint / 2,
            "changed={changed} footprint={footprint}"
        );
        assert!(bench.host.lines().is_empty()); // nothing was run
        let form = bench
            .dialog
            .form_style(bench.host.document())
            .expect("the form reads as a style");
        assert_eq!(form.plan_faces, SurfaceFaces::Elevation);
    }

    #[test]
    fn the_set_words_say_each_changed_field_as_the_verb_reads_it() {
        let from = SurfaceDisplayStyle {
            name: String::from("x"),
            ..SurfaceDisplayStyle::default()
        };
        assert_eq!(surface_style_set_words(&from, &from).unwrap(), "");
        let mut to = from.clone();
        to.description = String::from("two words");
        to.range_min = Some(2.5);
        to.stops = vec![
            ColourStop {
                value: 0.0,
                colour: Color::new(0, 0, 0, 255),
            },
            ColourStop {
                value: 10.0,
                colour: Color::new(200, 100, 50, 255),
            },
        ];
        to.shading = SurfaceStyle::ShadedWithEdges;
        assert_eq!(
            surface_style_set_words(&from, &to).unwrap(),
            "description=\"two words\" stops=0:#000000;10:#C86432 min=2.5 shading=edges"
        );
        // Clearing the range's start is the key with nothing after it.
        let back = surface_style_set_words(&to, &from)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(back.split(' ').any(|word| word == "min="), "{back}");
        to.description = String::from("a \"quoted\" word");
        assert!(surface_style_set_words(&from, &to).is_err());
    }

    #[test]
    fn its_lines_change_the_drawing_through_the_interpreter() {
        let mut bench = interpreted();
        assert!(select(&mut bench, "Style A"));
        bench.fill("surfaceStyleInterval", "0.5");
        bench.press("surfaceStyleApply");
        assert_eq!(
            style(bench.host.document(), "Style A").contours.interval,
            0.5
        );
        // One step, undone like a typed one.
        {
            let parts = &mut bench.host.parts;
            parts
                .interpreter
                .run(&mut parts.document, "UNDO")
                .unwrap_or_else(|error| panic!("{}", error.describe()));
        }
        assert_eq!(
            style(bench.host.document(), "Style A").contours.interval,
            1.0
        );
        // The dialog follows the drawing.
        bench.draw();
        assert_eq!(bench.text("surfaceStyleInterval"), "1");
    }

    #[test]
    fn a_description_with_backslashes_arrives_as_it_was_written_in_the_box() {
        // The verb reads a description as a typed text, in which two backslashes are one and a
        // backslash before an n is a line break, so the box's own backslashes are doubled on the
        // way to the line. The cases: a path, two backslashes then an n (which would read as a
        // backslash and an n if doubled once too few), a text that ends in a backslash, a blank
        // and plain text.
        let mut document = Document::new(None);
        let mut interpreter = CommandInterpreter::new();
        interpreter
            .run(&mut document, "SURFACESTYLE NEW x")
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        let mut from = style(&document, "x").clone();
        for description in ["C:\\dir", "a\\\\nb", "x\\", "two words", "plain"] {
            let mut to = from.clone();
            to.description = String::from(description);
            let words = surface_style_set_words(&from, &to)
                .unwrap_or_else(|error| panic!("{description}: {}", error.describe()));
            interpreter
                .run(&mut document, &format!("SURFACESTYLE SET x {words}"))
                .unwrap_or_else(|error| panic!("{description}: {}", error.describe()));
            assert_eq!(style(&document, "x").description, description);
            from = style(&document, "x").clone();
        }
        // Words the box can hold that a line cannot carry are refused with a sentence.
        let mut to = from.clone();
        to.description = String::from("a\nb");
        assert!(surface_style_set_words(&from, &to).is_err());
    }

    #[test]
    fn a_style_holding_more_than_the_forms_boxes_show_is_left_alone_until_the_box_is_moved() {
        // The drawing's own: "Big" holds a major every 2 000 000 intervals (past the box's
        // 1 000 000, which clamps) and faces3d=none, which a project can hold and the 3D list has
        // no word for (it shows flat). Nothing was edited, and the form said it had:
        // major=1000000 faces3d=flat - and Apply wrote them over the style.
        let mut host = fixed_host(ok("done"));
        prepare(&mut host);
        let mut set = host.document().surface_styles().clone();
        set.styles.push(SurfaceDisplayStyle {
            name: String::from("Big"),
            contours: SurfaceContours {
                major_every: 2_000_000,
                ..SurfaceContours::default()
            },
            faces3d: SurfaceFaces::None,
            ..SurfaceDisplayStyle::default()
        });
        host.document_mut()
            .set_surface_styles(set, "TEST")
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        let mut bench = bench(host);
        assert!(select(&mut bench, "Big"));
        assert!(apply_line(&bench).is_empty(), "{}", apply_line(&bench));
        assert!(!bench.enabled("surfaceStyleApply"));
        assert!(bench.text("surfaceStyleLine").contains("nothing to apply"));
        // An edit of something else carries that alone.
        bench.tick("surfaceStyleTriangles", true);
        assert_eq!(apply_line(&bench), "SURFACESTYLE SET Big triangles=on");
        // Moving the box is an edit, and carries its value.
        bench.tick("surfaceStyleTriangles", false);
        bench.fill("surfaceStyleFaces3d", "slope");
        assert_eq!(apply_line(&bench), "SURFACESTYLE SET Big faces3d=slope");
    }

    #[test]
    fn a_refused_name_leaves_the_style_being_edited_chosen_with_its_unapplied_edits() {
        // "Style A" with its interval edited to 0.25 and not applied; Rename, New and Copy to the
        // name "Style B", which is taken. Each is refused - and the list jumped to the style that
        // holds the name, whose values replaced the form's: the edit lost, and a following Apply
        // would edit the other style.
        let mut bench = interpreted();
        for button in ["surfaceStyleRename", "surfaceStyleNew", "surfaceStyleCopy"] {
            assert!(select(&mut bench, "Style A"));
            bench.fill("surfaceStyleInterval", "0.25");
            bench.fill("surfaceStyleName", "Style B");
            bench.press(button);
            assert!(
                bench.text("surfaceStyleStatus").contains("AlreadyExists"),
                "{button}"
            );
            assert_eq!(bench.dialog.current_style(), "Style A", "{button}");
            assert_eq!(bench.text("surfaceStyleInterval"), "0.25", "{button}");
            assert_eq!(
                apply_line(&bench),
                "SURFACESTYLE SET \"Style A\" interval=0.25",
                "{button}"
            );
        }
        // An accepted one still chooses the style it made.
        bench.fill("surfaceStyleName", "Style C");
        bench.press("surfaceStyleCopy");
        assert_eq!(bench.dialog.current_style(), "Style C");
    }

    #[test]
    fn the_bands_box_holds_the_same_range_as_the_style_check_so_no_typed_count_is_clamped() {
        // The form's box, the SURFACESTYLE line and a project file hold one range
        // (MAXIMUM_SURFACE_BANDS): the box was 2 to 100 while the check allowed a million, and a
        // style the verb made with 500 bands was written back as 100 by an Apply that changed only
        // its description.
        let mut bench = recorded();
        assert!(bench.has("surfaceStyleBands"));
        let range = surface_style_bands_range();
        assert_eq!(*range.start(), 2);
        assert_eq!(*range.end(), i64::from(MAXIMUM_SURFACE_BANDS));
        // The box itself takes both ends of that range, and nothing past it.
        bench.fill("surfaceStyleBands", "2");
        assert_eq!(bench.value("surfaceStyleBands"), "2");
        let most = format!("{MAXIMUM_SURFACE_BANDS}");
        bench.fill("surfaceStyleBands", &most);
        assert_eq!(bench.value("surfaceStyleBands"), most);
        {
            let Bench {
                harness,
                dialog,
                host,
            } = &mut bench;
            harness.fill(
                "surfaceStyleBands",
                &format!("{}", MAXIMUM_SURFACE_BANDS + 1),
            );
            let past = harness.run(|ui| dialog.draw(ui, &mut *host));
            assert!(past.is_err(), "a count past the range was taken");
        }
        // A count past it is refused by the check, so the box never had to clamp.
        let mut many = SurfaceDisplayStyle {
            name: String::from("Many"),
            bands: MAXIMUM_SURFACE_BANDS + 1,
            ..SurfaceDisplayStyle::default()
        };
        assert!(check_surface_style(&many).is_err());
        many.bands = MAXIMUM_SURFACE_BANDS;
        assert!(check_surface_style(&many).is_ok());
    }
}
